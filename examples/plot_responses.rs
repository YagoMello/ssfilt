//! Generates end-to-end response plots through the public streaming API.
//!
//! Usage:
//! cargo run --release --example `plot_responses` -- [--order ORDER] [--output PATH] [--csv PATH]
//! Legacy positional form: [order] [output.svg]

#[path = "plot_responses/analytic.rs"]
mod analytic;
#[path = "plot_responses/measure.rs"]
mod measure;
#[path = "plot_responses/render.rs"]
mod render;
#[path = "plot_responses/report.rs"]
mod report;
#[path = "plot_responses/simulation.rs"]
mod simulation;
#[path = "support/svg_smoothing.rs"]
mod svg_smoothing;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use analytic::{exact_frequency_point, frequency_ratios, phase_equalized_frequency_point};
use anyhow::Context;
use clap::{Parser, ValueEnum, error::ErrorKind};
use measure::{
    DEFAULT_SAMPLES_PER_CYCLE, MeasuredFrequencyPoint, SampleSchedule,
    measure_butterworth_low_pass_pair, measure_low_pass,
};
use plotters::prelude::RGBColor;
use render::{PhaseEqualizedData, ResponseData, ResponseSpec};
use simulation::{measure_phase_equalized_step_response, measure_step_response};
use ssfilt::{InputModel, LowPass, MAX_BESSEL_ORDER, Response};

macro_rules! dispatch_order {
    ($order:expr, $output:expr, $csv:expr, $configuration:expr; $($supported:literal),+ $(,)?) => {
        match $order {
            $($supported => plot::<$supported>($output, $csv, $configuration),)+
            unsupported => Err(format!(
                "order must be between 1 and {}, got {unsupported}",
                dispatch_order!(@last $($supported),+)
            ).into()),
        }
    };
    (@last $single:literal) => { $single };
    (@last $head:literal, $($tail:literal),+) => { dispatch_order!(@last $($tail),+) };
}

const MEASUREMENT_FREQUENCY_RATIOS: [f64; 18] = [
    0.05, 0.1, 0.25, 0.5, 0.7, 0.8, 0.9, 0.95, 1.0, 1.05, 1.1, 1.25, 1.5, 2.0, 3.0, 5.0, 10.0, 20.0,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum TopologyArg {
    LowPass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ResponseArg {
    All,
    RepeatedPole,
    Butterworth,
    Bessel,
    Chebyshev1,
}

#[derive(Debug, Parser)]
#[command(
    name = "plot_responses",
    about = "Generate response plots through the public streaming API.",
    after_help = "Legacy positional form: [ORDER] [OUTPUT.SVG]\nDefaults: order 4, target/filter-responses.svg, and a sibling .csv file."
)]
struct Cli {
    /// Legacy positional filter order; prefer `--order`.
    #[arg(
        value_name = "ORDER",
        conflicts_with = "order",
        value_parser = parse_order
    )]
    legacy_order: Option<usize>,
    /// Legacy positional SVG path; prefer `--output`.
    #[arg(value_name = "OUTPUT.SVG", conflicts_with = "output")]
    legacy_output: Option<PathBuf>,
    /// Filter order from 1 through 50.
    #[arg(
        long,
        value_name = "ORDER",
        conflicts_with = "legacy_order",
        value_parser = parse_order
    )]
    order: Option<usize>,
    /// SVG dashboard output path.
    #[arg(long, value_name = "PATH", conflicts_with = "legacy_output")]
    output: Option<PathBuf>,
    /// CSV measurement report path; defaults beside the SVG.
    #[arg(long, value_name = "PATH")]
    csv: Option<PathBuf>,
    /// Filter topology to plot.
    #[arg(long, value_enum, default_value_t = TopologyArg::LowPass)]
    topology: TopologyArg,
    /// Response family to plot, or all available families.
    #[arg(long, value_enum, default_value_t = ResponseArg::All)]
    response: ResponseArg,
    /// Overlay the validated Butterworth phase-equalized response (orders 4 and 8 only).
    #[arg(long)]
    phase_equalized: bool,
}

#[derive(Debug)]
struct Arguments {
    order: usize,
    output_path: PathBuf,
    csv_path: PathBuf,
    topology: TopologyArg,
    response: ResponseArg,
    phase_equalized: bool,
}

fn parse_order(value: &str) -> Result<usize, String> {
    let order = value
        .parse::<usize>()
        .map_err(|_| format!("order must be an integer from 1 through 50, got {value:?}"))?;
    if (1..=50).contains(&order) {
        Ok(order)
    } else {
        Err(format!("order must be between 1 and 50, got {order}"))
    }
}

impl Cli {
    fn normalize(self) -> Result<Arguments, clap::Error> {
        if self.legacy_order.is_some() && self.order.is_some() {
            return Err(clap::Error::raw(
                ErrorKind::ArgumentConflict,
                "positional ORDER conflicts with --order",
            ));
        }
        if self.legacy_output.is_some() && self.output.is_some() {
            return Err(clap::Error::raw(
                ErrorKind::ArgumentConflict,
                "positional OUTPUT.SVG conflicts with --output",
            ));
        }

        let order = self.order.or(self.legacy_order).unwrap_or(4);
        let output_path = self
            .output
            .or(self.legacy_output)
            .unwrap_or_else(|| PathBuf::from("target/filter-responses.svg"));
        let csv_path = self
            .csv
            .unwrap_or_else(|| output_path.with_extension("csv"));
        if csv_path == output_path {
            return Err(clap::Error::raw(
                ErrorKind::ArgumentConflict,
                "SVG and CSV output paths must be different",
            ));
        }
        if self.phase_equalized && !matches!(order, 4 | 8) {
            return Err(clap::Error::raw(
                ErrorKind::InvalidValue,
                "--phase-equalized is validated only for Butterworth orders 4 and 8",
            ));
        }
        if self.phase_equalized
            && !matches!(self.response, ResponseArg::All | ResponseArg::Butterworth)
        {
            return Err(clap::Error::raw(
                ErrorKind::ArgumentConflict,
                "--phase-equalized requires --response all or --response butterworth",
            ));
        }

        Ok(Arguments {
            order,
            output_path,
            csv_path,
            topology: self.topology,
            response: self.response,
            phase_equalized: self.phase_equalized,
        })
    }
}

fn main() -> anyhow::Result<()> {
    let arguments = Cli::parse().normalize()?;
    create_parent_directory(&arguments.output_path).with_context(|| {
        format!(
            "creating the parent directory for SVG output {}",
            arguments.output_path.display()
        )
    })?;
    create_parent_directory(&arguments.csv_path).with_context(|| {
        format!(
            "creating the parent directory for CSV output {}",
            arguments.csv_path.display()
        )
    })?;

    let configuration = PlotConfiguration {
        topology: arguments.topology,
        response: arguments.response,
        phase_equalized: arguments.phase_equalized,
    };
    dispatch_order!(arguments.order, &arguments.output_path, &arguments.csv_path, configuration;
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50
    )
    .map_err(anyhow::Error::from_boxed)
    .with_context(|| {
        format!(
            "generating order {} response plots (SVG: {}, CSV: {})",
            arguments.order,
            arguments.output_path.display(),
            arguments.csv_path.display()
        )
    })?;

    println!("wrote {}", arguments.output_path.display());
    println!("wrote {}", arguments.csv_path.display());
    Ok(())
}

fn create_parent_directory(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn plot<const N: usize>(
    output_path: &Path,
    csv_path: &Path,
    configuration: PlotConfiguration,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let specs = selected_specs::<N>(configuration)?;

    let mut responses = Vec::with_capacity(specs.len());
    for spec in specs {
        println!("evaluating {}", spec.label);
        let frequency = frequency_ratios()
            .map(|ratio| exact_frequency_point::<N>(spec.response, ratio))
            .collect::<Result<Vec<_>, _>>()?;
        let include_equalized =
            configuration.phase_equalized && spec.response == Response::Butterworth;
        let mut measurements = Vec::with_capacity(MEASUREMENT_FREQUENCY_RATIOS.len() * 2);
        let mut equalized_measurements =
            include_equalized.then(|| Vec::with_capacity(MEASUREMENT_FREQUENCY_RATIOS.len() * 2));
        for ratio in MEASUREMENT_FREQUENCY_RATIOS {
            let analytic = exact_frequency_point::<N>(spec.response, ratio)?;
            let equalized_analytic = include_equalized
                .then(|| phase_equalized_frequency_point::<N>(ratio))
                .transpose()?;
            for schedule in [SampleSchedule::Uniform, SampleSchedule::BoundedJitter] {
                if include_equalized {
                    let pair = measure_butterworth_low_pass_pair::<N>(
                        ratio,
                        schedule,
                        DEFAULT_SAMPLES_PER_CYCLE,
                    )?;
                    measurements.push(MeasuredFrequencyPoint::new(
                        pair.source,
                        analytic.gain_db,
                        analytic.phase_radians,
                        analytic.normalized_group_delay,
                    ));
                    let equalized_analytic = equalized_analytic
                        .as_ref()
                        .expect("equalized analytics accompany paired measurements");
                    equalized_measurements
                        .as_mut()
                        .expect("equalized storage accompanies paired measurements")
                        .push(MeasuredFrequencyPoint::new(
                            pair.equalized,
                            equalized_analytic.gain_db,
                            equalized_analytic.phase_radians,
                            equalized_analytic.normalized_group_delay,
                        ));
                } else {
                    let streamed = measure_low_pass::<N>(
                        spec.response,
                        ratio,
                        schedule,
                        DEFAULT_SAMPLES_PER_CYCLE,
                    )?;
                    measurements.push(MeasuredFrequencyPoint::new(
                        streamed,
                        analytic.gain_db,
                        analytic.phase_radians,
                        analytic.normalized_group_delay,
                    ));
                }
            }
        }
        let group_delay = frequency
            .iter()
            .map(|point| (point.ratio, point.normalized_group_delay))
            .collect();
        let step = measure_step_response::<N>(spec.response)?;
        let phase_equalized = if include_equalized {
            let metadata = LowPass::<N>::builder(1.0)
                .response(Response::Butterworth)
                .input_model(InputModel::Linear)
                .build_phase_equalized()?;
            let frequency = frequency_ratios()
                .map(phase_equalized_frequency_point::<N>)
                .collect::<Result<Vec<_>, _>>()?;
            Some(PhaseEqualizedData {
                frequency,
                measurements: equalized_measurements
                    .expect("equalized measurements were collected"),
                step: measure_phase_equalized_step_response::<N>()?,
                validated_band_edge_ratio: metadata.validated_band_edge_hz(),
                target_normalized_delay: core::f64::consts::TAU
                    * metadata.target_constant_delay_seconds(),
                mean_added_normalized_delay: core::f64::consts::TAU
                    * metadata.mean_added_delay_seconds(),
            })
        } else {
            None
        };
        responses.push(ResponseData {
            spec,
            frequency,
            measurements,
            group_delay,
            step,
            phase_equalized,
        });
    }

    render::draw::<N>(output_path, &responses)?;
    svg_smoothing::smooth_svg_curves(output_path)?;
    report::write_csv(csv_path, &responses)?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct PlotConfiguration {
    topology: TopologyArg,
    response: ResponseArg,
    phase_equalized: bool,
}

fn selected_specs<const N: usize>(
    configuration: PlotConfiguration,
) -> Result<Vec<ResponseSpec>, Box<dyn Error + Send + Sync>> {
    debug_assert_eq!(configuration.topology, TopologyArg::LowPass);
    let mut specs = vec![
        ResponseSpec {
            label: "Repeated pole",
            response: Response::RepeatedPole,
            color: RGBColor(46, 111, 214),
        },
        ResponseSpec {
            label: "Butterworth",
            response: Response::Butterworth,
            color: RGBColor(224, 91, 74),
        },
        ResponseSpec {
            label: "Bessel",
            response: Response::Bessel,
            color: RGBColor(135, 82, 161),
        },
        ResponseSpec {
            label: "Chebyshev I, 0.5 dB ripple",
            response: Response::Chebyshev1 { ripple_db: 0.5 },
            color: RGBColor(36, 157, 92),
        },
    ];
    specs.retain(|spec| match configuration.response {
        ResponseArg::All => true,
        ResponseArg::RepeatedPole => spec.response == Response::RepeatedPole,
        ResponseArg::Butterworth => spec.response == Response::Butterworth,
        ResponseArg::Bessel => spec.response == Response::Bessel,
        ResponseArg::Chebyshev1 => matches!(spec.response, Response::Chebyshev1 { .. }),
    });
    if N > MAX_BESSEL_ORDER && configuration.response == ResponseArg::Bessel {
        return Err(format!("Bessel responses support orders through {MAX_BESSEL_ORDER}").into());
    }
    if N > MAX_BESSEL_ORDER {
        specs.retain(|spec| spec.response != Response::Bessel);
        println!("omitting Bessel: supported through order {MAX_BESSEL_ORDER}");
    }
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{Parser, error::ErrorKind};

    use super::{
        Arguments, Cli, MEASUREMENT_FREQUENCY_RATIOS, PlotConfiguration, ResponseArg, TopologyArg,
        selected_specs,
    };

    fn args(values: &[&str]) -> Result<Arguments, String> {
        let mut argv = vec!["plot_responses"];
        argv.extend_from_slice(values);
        let cli = Cli::try_parse_from(argv).map_err(|error| error.to_string())?;
        cli.normalize().map_err(|error| error.to_string())
    }

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn arguments_keep_positional_compatibility_and_derive_csv_path() {
        let arguments = args(&["8", "out/response.svg"]).unwrap();
        assert_eq!(arguments.order, 8);
        assert_eq!(arguments.output_path, path("out/response.svg"));
        assert_eq!(arguments.csv_path, path("out/response.csv"));
    }

    #[test]
    fn explicit_csv_path_may_appear_before_positionals() {
        let arguments = args(&["--csv", "measurements/data.csv", "3", "plot.svg"]).unwrap();
        assert_eq!(arguments.order, 3);
        assert_eq!(arguments.output_path, path("plot.svg"));
        assert_eq!(arguments.csv_path, path("measurements/data.csv"));
    }

    #[test]
    fn named_options_and_defaults_normalize_to_the_expected_paths() {
        let arguments = args(&[
            "--order",
            "3",
            "--output",
            "out/response.svg",
            "--csv",
            "reports/data.csv",
            "--topology",
            "low-pass",
            "--response",
            "butterworth",
        ])
        .unwrap();
        assert_eq!(arguments.order, 3);
        assert_eq!(arguments.output_path, path("out/response.svg"));
        assert_eq!(arguments.csv_path, path("reports/data.csv"));
        assert_eq!(arguments.topology, TopologyArg::LowPass);
        assert_eq!(arguments.response, ResponseArg::Butterworth);

        let defaults = args(&[]).unwrap();
        assert_eq!(defaults.order, 4);
        assert_eq!(defaults.output_path, path("target/filter-responses.svg"));
        assert_eq!(defaults.csv_path, path("target/filter-responses.csv"));
        assert_eq!(defaults.topology, TopologyArg::LowPass);
        assert_eq!(defaults.response, ResponseArg::All);
        assert!(!defaults.phase_equalized);
    }

    #[test]
    fn named_options_conflict_with_their_legacy_positionals() {
        for values in [
            vec!["4", "--order", "8"],
            vec!["4", "plot.svg", "--output", "named.svg"],
        ] {
            assert!(args(&values).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn help_is_generated_with_named_and_legacy_forms() {
        let error = Cli::try_parse_from(["plot_responses", "--help"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let help = error.to_string();
        assert!(help.contains("--order"));
        assert!(help.contains("--output"));
        assert!(help.contains("--csv"));
        assert!(help.contains("--topology"));
        assert!(help.contains("--response"));
        assert!(help.contains("--phase-equalized"));
        assert!(help.contains("Filter order from 1 through 50"));
        assert!(help.contains("SVG dashboard output path"));
        assert!(help.contains("CSV measurement report path"));
        assert!(help.contains("Legacy positional form"));
        assert!(help.contains("target/filter-responses.svg"));
    }

    #[test]
    fn order_range_is_rejected_during_parsing_for_named_and_legacy_forms() {
        for values in [
            vec!["1"],
            vec!["50"],
            vec!["--order", "1"],
            vec!["--order", "50"],
        ] {
            assert!(
                args(&values).is_ok(),
                "rejected inclusive boundary {values:?}"
            );
        }
        for values in [
            vec!["0"],
            vec!["51"],
            vec!["--order", "0"],
            vec!["--order", "51"],
        ] {
            let error = args(&values).unwrap_err();
            assert!(
                error.contains("between 1 and 50"),
                "unexpected error: {error}"
            );
        }
    }

    #[test]
    fn csv_cannot_overwrite_the_svg_output() {
        let error = args(&["4", "same-path", "--csv", "same-path"]).unwrap_err();
        assert!(error.contains("SVG and CSV output paths must be different"));
    }

    #[test]
    fn response_selection_is_explicit_and_never_silently_drops_requested_bessel() {
        let butterworth = selected_specs::<4>(PlotConfiguration {
            topology: TopologyArg::LowPass,
            response: ResponseArg::Butterworth,
            phase_equalized: false,
        })
        .unwrap();
        assert_eq!(butterworth.len(), 1);
        assert_eq!(butterworth[0].response, ssfilt::Response::Butterworth);

        let all_high_order = selected_specs::<26>(PlotConfiguration {
            topology: TopologyArg::LowPass,
            response: ResponseArg::All,
            phase_equalized: false,
        })
        .unwrap();
        assert_eq!(all_high_order.len(), 3);
        assert!(
            all_high_order
                .iter()
                .all(|spec| spec.response != ssfilt::Response::Bessel)
        );

        let requested_bessel = selected_specs::<26>(PlotConfiguration {
            topology: TopologyArg::LowPass,
            response: ResponseArg::Bessel,
            phase_equalized: false,
        })
        .err()
        .expect("an explicitly unsupported Bessel order must fail")
        .to_string();
        assert!(requested_bessel.contains("orders through 25"));
    }

    #[test]
    fn invalid_topology_and_response_values_are_rejected_by_clap() {
        for values in [
            vec!["--topology", "band-pass"],
            vec!["--response", "elliptic"],
        ] {
            assert!(args(&values).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn phase_equalized_overlay_accepts_only_its_publicly_validated_cases() {
        for values in [
            vec!["--order", "4", "--phase-equalized"],
            vec![
                "--order",
                "8",
                "--response",
                "butterworth",
                "--phase-equalized",
            ],
        ] {
            assert!(args(&values).unwrap().phase_equalized);
        }

        for values in [
            vec!["--order", "3", "--phase-equalized"],
            vec!["--order", "16", "--phase-equalized"],
            vec!["--order", "4", "--response", "bessel", "--phase-equalized"],
        ] {
            assert!(args(&values).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn streamed_validation_ratios_span_the_complete_plotted_frequency_band() {
        assert!(MEASUREMENT_FREQUENCY_RATIOS.is_sorted());
        assert_eq!(MEASUREMENT_FREQUENCY_RATIOS.last(), Some(&20.0));
        for expected in [1.25, 1.5, 2.0, 3.0, 5.0, 10.0, 20.0] {
            assert!(MEASUREMENT_FREQUENCY_RATIOS.contains(&expected));
        }
    }
}
