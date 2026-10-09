//! Generates end-to-end response plots through the public streaming API.
//!
//! Usage:
//! cargo run --release --example plot_responses -- [order] [output.svg]

#[path = "plot_responses/analytic.rs"]
mod analytic;
#[path = "plot_responses/render.rs"]
mod render;
#[path = "plot_responses/simulation.rs"]
mod simulation;
#[path = "plot_responses/svg_smoothing.rs"]
mod svg_smoothing;

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use analytic::{exact_frequency_point, frequency_ratios, group_delay};
use plotters::prelude::RGBColor;
use render::{ResponseData, ResponseSpec};
use simulation::measure_step_response;
use ssfilt::{MAX_BESSEL_ORDER, Response};

macro_rules! dispatch_order {
    ($order:expr, $output:expr; $($supported:literal),+ $(,)?) => {
        match $order {
            $($supported => plot::<$supported>($output),)+
            unsupported => Err(format!(
                "order must be between 1 and {}, got {unsupported}",
                dispatch_order!(@last $($supported),+)
            ).into()),
        }
    };
    (@last $single:literal) => { $single };
    (@last $head:literal, $($tail:literal),+) => { dispatch_order!(@last $($tail),+) };
}

fn main() -> Result<(), Box<dyn Error>> {
    let (order, output_path) = arguments()?;
    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    dispatch_order!(order, &output_path;
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50
    )?;

    println!("wrote {}", output_path.display());
    Ok(())
}

fn arguments() -> Result<(usize, PathBuf), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let order = arguments.next().map_or(Ok(4), |value| value.parse())?;
    let output = arguments.next().map_or_else(
        || PathBuf::from("target/filter-responses.svg"),
        PathBuf::from,
    );
    if let Some(argument) = arguments.next() {
        return Err(format!("unexpected argument: {argument}").into());
    }
    Ok((order, output))
}

fn plot<const N: usize>(output_path: &Path) -> Result<(), Box<dyn Error>> {
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
        let group_delay = group_delay(&frequency);
        let step = measure_step_response::<N>(spec.response)?;
        responses.push(ResponseData {
            spec,
            frequency,
            group_delay,
            step,
        });
    }

    render::draw::<N>(output_path, &responses)?;
    svg_smoothing::smooth_svg_response_curves(output_path)?;
    Ok(())
}
