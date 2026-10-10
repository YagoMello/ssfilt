//! Generates end-to-end response plots through the public streaming API.
//!
//! Usage:
//! cargo run --release --example `plot_responses` -- [order] [output.svg] [--csv output.csv]

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
#[path = "plot_responses/svg_smoothing.rs"]
mod svg_smoothing;

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use analytic::{exact_frequency_point, frequency_ratios, group_delay};
use measure::{
    DEFAULT_SAMPLES_PER_CYCLE, MeasuredFrequencyPoint, SampleSchedule, measure_low_pass,
};
use plotters::prelude::RGBColor;
use render::{ResponseData, ResponseSpec};
use simulation::measure_step_response;
use ssfilt::{MAX_BESSEL_ORDER, Response};

macro_rules! dispatch_order {
    ($order:expr, $output:expr, $csv:expr; $($supported:literal),+ $(,)?) => {
        match $order {
            $($supported => plot::<$supported>($output, $csv),)+
            unsupported => Err(format!(
                "order must be between 1 and {}, got {unsupported}",
                dispatch_order!(@last $($supported),+)
            ).into()),
        }
    };
    (@last $single:literal) => { $single };
    (@last $head:literal, $($tail:literal),+) => { dispatch_order!(@last $($tail),+) };
}

const MEASUREMENT_FREQUENCY_RATIOS: [f64; 11] =
    [0.05, 0.1, 0.25, 0.5, 0.7, 0.8, 0.9, 0.95, 1.0, 1.05, 1.1];

struct Arguments {
    order: usize,
    output_path: PathBuf,
    csv_path: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(arguments) = arguments()? else {
        return Ok(());
    };
    create_parent_directory(&arguments.output_path)?;
    create_parent_directory(&arguments.csv_path)?;

    dispatch_order!(arguments.order, &arguments.output_path, &arguments.csv_path;
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50
    )?;

    println!("wrote {}", arguments.output_path.display());
    println!("wrote {}", arguments.csv_path.display());
    Ok(())
}

fn arguments() -> Result<Option<Arguments>, Box<dyn Error>> {
    parse_arguments(env::args().skip(1))
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Option<Arguments>, Box<dyn Error>> {
    let mut positional = Vec::new();
    let mut csv_path = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("usage: plot_responses [order] [output.svg] [--csv output.csv]");
                println!("defaults: order 4, target/filter-responses.svg, sibling .csv file");
                return Ok(None);
            }
            "--csv" => {
                let value = arguments.next().ok_or("--csv requires a path")?;
                if csv_path.replace(PathBuf::from(value)).is_some() {
                    return Err("--csv may be supplied only once".into());
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into());
            }
            value => positional.push(value.to_owned()),
        }
    }
    if positional.len() > 2 {
        return Err(format!("unexpected argument: {}", positional[2]).into());
    }
    let order = positional.first().map_or(Ok(4), |value| value.parse())?;
    let output_path = positional.get(1).map_or_else(
        || PathBuf::from("target/filter-responses.svg"),
        PathBuf::from,
    );
    let csv_path = csv_path.unwrap_or_else(|| output_path.with_extension("csv"));
    if csv_path == output_path {
        return Err("SVG and CSV output paths must be different".into());
    }
    Ok(Some(Arguments {
        order,
        output_path,
        csv_path,
    }))
}

fn create_parent_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn plot<const N: usize>(output_path: &Path, csv_path: &Path) -> Result<(), Box<dyn Error>> {
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
    if N > MAX_BESSEL_ORDER {
        specs.retain(|spec| spec.response != Response::Bessel);
        println!("omitting Bessel: supported through order {MAX_BESSEL_ORDER}");
    }

    let mut responses = Vec::with_capacity(specs.len());
    for spec in specs {
        println!("evaluating {}", spec.label);
        let frequency = frequency_ratios()
            .map(|ratio| exact_frequency_point::<N>(spec.response, ratio))
            .collect::<Result<Vec<_>, _>>()?;
        let mut measurements = Vec::with_capacity(MEASUREMENT_FREQUENCY_RATIOS.len() * 2);
        for ratio in MEASUREMENT_FREQUENCY_RATIOS {
            let analytic = exact_frequency_point::<N>(spec.response, ratio)?;
            for schedule in [SampleSchedule::Uniform, SampleSchedule::BoundedJitter] {
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
                ));
            }
        }
        let group_delay = group_delay(&frequency);
        let step = measure_step_response::<N>(spec.response)?;
        responses.push(ResponseData {
            spec,
            frequency,
            measurements,
            group_delay,
            step,
        });
    }

    render::draw::<N>(output_path, &responses)?;
    svg_smoothing::smooth_svg_response_curves(output_path)?;
    report::write_csv(csv_path, &responses)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse_arguments;

    #[test]
    fn arguments_keep_positional_compatibility_and_derive_csv_path() {
        let arguments = parse_arguments(["8".to_owned(), "out/response.svg".to_owned()])
            .unwrap()
            .unwrap();
        assert_eq!(arguments.order, 8);
        assert_eq!(arguments.output_path, PathBuf::from("out/response.svg"));
        assert_eq!(arguments.csv_path, PathBuf::from("out/response.csv"));
    }

    #[test]
    fn explicit_csv_path_may_appear_before_positionals() {
        let arguments = parse_arguments([
            "--csv".to_owned(),
            "measurements/data.csv".to_owned(),
            "3".to_owned(),
            "plot.svg".to_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(arguments.order, 3);
        assert_eq!(arguments.output_path, PathBuf::from("plot.svg"));
        assert_eq!(arguments.csv_path, PathBuf::from("measurements/data.csv"));
    }

    #[test]
    fn csv_cannot_overwrite_the_svg_output() {
        let error = parse_arguments([
            "4".to_owned(),
            "same-path".to_owned(),
            "--csv".to_owned(),
            "same-path".to_owned(),
        ])
        .err()
        .unwrap();
        assert_eq!(
            error.to_string(),
            "SVG and CSV output paths must be different"
        );
    }
}
