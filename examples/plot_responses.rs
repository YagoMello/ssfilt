//! Plots analytic response curves and measurements made through the public
//! streaming API.
//!
//! Usage: `cargo run --release --example plot_responses -- [ORDER] [OUTPUT.svg]`
//! Use `--topology` and `--response` to select a view. The positional form is
//! retained for existing low-pass invocations.

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
#[path = "support/transfer.rs"]
mod transfer;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use render::{ResponseData, ResponseSpec};
use ssfilt::{MAX_BESSEL_BAND_PASS_ORDER, MAX_BESSEL_ORDER, Response};

const MEASUREMENT_FREQUENCY_RATIOS: [f64; 15] = [
    0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0, 3.0, 5.0, 10.0, 20.0,
];

macro_rules! dispatch_order {
    ($order:expr, $arguments:expr; $($supported:literal),+ $(,)?) => {
        match $order {
            $($supported => plot::<$supported>($arguments),)+
            unsupported => bail!("order must be between 1 and 50, got {unsupported}"),
        }
    };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
#[allow(clippy::enum_variant_names)]
enum Topology {
    #[default]
    LowPass,
    HighPass,
    BandPass,
}

impl Topology {
    const fn label(self) -> &'static str {
        match self {
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::BandPass => "band-pass",
        }
    }

    const fn prototype_order(self, final_order: usize) -> usize {
        match self {
            Self::LowPass | Self::HighPass => final_order,
            Self::BandPass => final_order / 2,
        }
    }

    const fn bessel_limit(self) -> usize {
        match self {
            Self::LowPass | Self::HighPass => MAX_BESSEL_ORDER,
            Self::BandPass => MAX_BESSEL_BAND_PASS_ORDER,
        }
    }

    const fn steady_step_value(self) -> f64 {
        match self {
            Self::LowPass => 1.0,
            Self::HighPass | Self::BandPass => 0.0,
        }
    }

    const fn minus_three_db_edges(self) -> (f64, f64) {
        match self {
            Self::LowPass | Self::HighPass => (1.0, 1.0),
            Self::BandPass => (0.5, 2.0),
        }
    }

    const fn frequency_reference(self) -> &'static str {
        match self {
            Self::LowPass | Self::HighPass => "whole-filter −3 dB cutoff",
            Self::BandPass => "geometric band center",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum ResponseChoice {
    #[default]
    All,
    RepeatedPole,
    Butterworth,
    Bessel,
    Chebyshev1,
}

#[derive(Debug, Parser)]
#[command(
    name = "plot_responses",
    about = "Plot filter responses and measurements from the public streaming API.",
    after_help = "Legacy positional form: [ORDER] [OUTPUT.svg]\nDefaults: order 4, low-pass, all supported response families, target/filter-responses.svg, and a sibling CSV file. Band-pass order is the final even order; its default −3 dB edges are 0.5 Hz and 2 Hz."
)]
struct Cli {
    /// Legacy positional order. Prefer --order.
    #[arg(value_name = "ORDER", conflicts_with = "order", value_parser = parse_order)]
    legacy_order: Option<usize>,
    /// Legacy positional SVG output path. Prefer --output.
    #[arg(value_name = "OUTPUT.svg", conflicts_with = "output")]
    legacy_output: Option<PathBuf>,
    /// Final filter order from 1 through 50.
    #[arg(long, value_name = "ORDER", conflicts_with = "legacy_order", value_parser = parse_order)]
    order: Option<usize>,
    /// SVG output path.
    #[arg(long, value_name = "PATH", conflicts_with = "legacy_output")]
    output: Option<PathBuf>,
    /// CSV measurement output path; defaults beside the SVG.
    #[arg(long, value_name = "PATH")]
    csv: Option<PathBuf>,
    /// Filter topology to inspect.
    #[arg(long, value_enum, default_value_t = Topology::LowPass)]
    topology: Topology,
    /// Response family to inspect, or all supported families.
    #[arg(long, value_enum, default_value_t = ResponseChoice::All)]
    response: ResponseChoice,
}

#[derive(Debug)]
struct Arguments {
    order: usize,
    output_path: PathBuf,
    csv_path: PathBuf,
    topology: Topology,
    response: ResponseChoice,
}

fn parse_order(value: &str) -> std::result::Result<usize, String> {
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
    fn normalize(self) -> Result<Arguments> {
        let order = self.order.or(self.legacy_order).unwrap_or(4);
        let output_path = self
            .output
            .or(self.legacy_output)
            .unwrap_or_else(|| default_output_path(self.topology));
        let csv_path = self
            .csv
            .unwrap_or_else(|| output_path.with_extension("csv"));

        if csv_path == output_path {
            bail!("SVG and CSV output paths must be different");
        }
        if self.topology == Topology::BandPass && !order.is_multiple_of(2) {
            bail!("band-pass final order must be positive and even, got {order}");
        }
        if self.response == ResponseChoice::Bessel && order > self.topology.bessel_limit() {
            bail!(
                "Bessel for {} supports final order up to {}; requested {order}",
                self.topology.label(),
                self.topology.bessel_limit()
            );
        }

        Ok(Arguments {
            order,
            output_path,
            csv_path,
            topology: self.topology,
            response: self.response,
        })
    }
}

fn default_output_path(topology: Topology) -> PathBuf {
    match topology {
        Topology::LowPass => PathBuf::from("target/filter-responses.svg"),
        Topology::HighPass | Topology::BandPass => {
            PathBuf::from(format!("target/{}-responses.svg", topology.label()))
        }
    }
}

fn main() -> Result<()> {
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

    dispatch_order!(arguments.order, &arguments;
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50
    )
    .with_context(|| {
        format!(
            "generating order {} {} response plots",
            arguments.order,
            arguments.topology.label()
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

fn response_specs(
    choice: ResponseChoice,
    topology: Topology,
    final_order: usize,
) -> Vec<ResponseSpec> {
    let choices = [
        ResponseChoice::RepeatedPole,
        ResponseChoice::Butterworth,
        ResponseChoice::Bessel,
        ResponseChoice::Chebyshev1,
    ];
    let mut specs = Vec::with_capacity(choices.len());
    for selected in choices {
        if choice != ResponseChoice::All && selected != choice {
            continue;
        }
        if selected == ResponseChoice::Bessel && final_order > topology.bessel_limit() {
            eprintln!(
                "omitting Bessel: {} supports final order up to {}",
                topology.label(),
                topology.bessel_limit()
            );
            continue;
        }
        specs.push(match selected {
            ResponseChoice::RepeatedPole => ResponseSpec::new(
                "Repeated pole",
                Response::RepeatedPole,
                plotters::prelude::RGBColor(46, 111, 214),
            ),
            ResponseChoice::Butterworth => ResponseSpec::new(
                "Butterworth",
                Response::Butterworth,
                plotters::prelude::RGBColor(224, 91, 74),
            ),
            ResponseChoice::Bessel => ResponseSpec::new(
                "Bessel",
                Response::Bessel,
                plotters::prelude::RGBColor(135, 82, 161),
            ),
            ResponseChoice::Chebyshev1 => ResponseSpec::new(
                "Chebyshev I, 0.5 dB ripple",
                Response::Chebyshev1 { ripple_db: 0.5 },
                plotters::prelude::RGBColor(36, 157, 92),
            ),
            ResponseChoice::All => unreachable!("all expands to its concrete response choices"),
        });
    }
    specs
}

fn plot<const N: usize>(arguments: &Arguments) -> Result<()> {
    let specs = response_specs(arguments.response, arguments.topology, N);
    let mut responses = Vec::with_capacity(specs.len());
    for spec in specs {
        println!("evaluating {} {}", spec.label, arguments.topology.label());
        let frequency = analytic::frequency_ratios(arguments.topology)
            .map(|ratio| analytic::frequency_point(arguments.topology, spec.response, N, ratio))
            .collect::<Result<Vec<_>>>()?;
        let mut measurements = Vec::with_capacity(MEASUREMENT_FREQUENCY_RATIOS.len());
        for ratio in MEASUREMENT_FREQUENCY_RATIOS {
            let analytic = analytic::frequency_point(arguments.topology, spec.response, N, ratio)?;
            let (measured, public_update_count, status) = match measure::measure::<N>(
                arguments.topology,
                spec.response,
                ratio,
                analytic,
            )? {
                measure::MeasurementResult::Measured {
                    point,
                    public_update_count,
                } => (
                    Some(point),
                    public_update_count,
                    measure::InspectionStatus::Measured,
                ),
                measure::MeasurementResult::WorkLimitExceeded {
                    public_update_count,
                    limit,
                    minimum_work_exceeds_limit,
                } => {
                    let (status, reason) = if minimum_work_exceeds_limit {
                        (
                            measure::InspectionStatus::WorkLimitEstimate,
                            "minimum warm-up and fit work exceeds the limit",
                        )
                    } else {
                        (
                            measure::InspectionStatus::WorkLimitReached,
                            "the public-update limit was reached",
                        )
                    };
                    eprintln!(
                        "skipping {} {} order {N} stream-inspection point at {ratio}× {}: {reason} ({limit} public updates)",
                        arguments.topology.label(),
                        spec.label,
                        arguments.topology.frequency_reference(),
                    );
                    (None, public_update_count, status)
                }
            };
            measurements.push(measure::InspectionPoint {
                ratio,
                analytic,
                measured,
                public_update_count,
                status,
            });
        }
        let step = simulation::measure_step_response::<N>(arguments.topology, spec.response)?;
        responses.push(ResponseData {
            spec,
            frequency,
            measurements,
            step,
        });
    }

    render::draw::<N>(
        arguments.output_path.as_path(),
        arguments.topology,
        &responses,
    )
    .with_context(|| format!("rendering SVG {}", arguments.output_path.display()))?;
    svg_smoothing::smooth_svg_curves(arguments.output_path.as_path()).with_context(|| {
        format!(
            "smoothing SVG curves in {}",
            arguments.output_path.display()
        )
    })?;
    report::write_csv(
        arguments.csv_path.as_path(),
        arguments.topology,
        N,
        &responses,
    )
    .with_context(|| format!("writing measurement CSV {}", arguments.csv_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser;

    use super::{Cli, ResponseChoice, Topology, default_output_path, response_specs};

    #[test]
    fn defaults_keep_the_legacy_low_pass_output_location() {
        let arguments = Cli::try_parse_from(["plot_responses"])
            .unwrap()
            .normalize()
            .unwrap();
        assert_eq!(arguments.order, 4);
        assert_eq!(arguments.topology, Topology::LowPass);
        assert_eq!(arguments.response, ResponseChoice::All);
        assert_eq!(
            arguments.output_path,
            default_output_path(Topology::LowPass)
        );
        assert_eq!(arguments.csv_path, Path::new("target/filter-responses.csv"));
    }

    #[test]
    fn positional_order_and_svg_path_remain_supported() {
        let arguments = Cli::try_parse_from(["plot_responses", "8", "target/custom.svg"])
            .unwrap()
            .normalize()
            .unwrap();
        assert_eq!(arguments.order, 8);
        assert_eq!(arguments.output_path, Path::new("target/custom.svg"));
        assert_eq!(arguments.csv_path, Path::new("target/custom.csv"));
    }

    #[test]
    fn band_pass_requires_an_even_final_order() {
        let error =
            Cli::try_parse_from(["plot_responses", "--topology", "band-pass", "--order", "5"])
                .unwrap()
                .normalize()
                .unwrap_err();
        assert!(error.to_string().contains("positive and even"));
    }

    #[test]
    fn explicitly_selected_bessel_reports_the_topology_limit() {
        let error = Cli::try_parse_from([
            "plot_responses",
            "--topology",
            "high-pass",
            "--response",
            "bessel",
            "--order",
            "26",
        ])
        .unwrap()
        .normalize()
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Bessel for high-pass supports final order up to 25")
        );
    }

    #[test]
    fn all_responses_keep_bessel_at_the_final_band_pass_limit() {
        let specs = response_specs(ResponseChoice::All, Topology::BandPass, 50);
        assert_eq!(specs.len(), 4);
        assert_eq!(specs[2].label, "Bessel");
    }

    #[test]
    fn all_low_pass_responses_omit_only_bessel_above_its_limit() {
        let specs = response_specs(ResponseChoice::All, Topology::LowPass, 50);
        assert_eq!(specs.len(), 3);
        assert!(specs.iter().all(|spec| spec.label != "Bessel"));
    }
}
