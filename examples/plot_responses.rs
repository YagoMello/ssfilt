//! Generates an end-to-end magnitude-response plot through the public API.
//!
//! Usage:
//! `cargo run --release --example plot_responses -- [order] [output.svg]`

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use num_traits::ToPrimitive;
use plotters::prelude::*;
use ssfilt::{InputModel, LowPass, Response};

const CUTOFF_HZ: f64 = 1.0;
const POINTS: u32 = 161;
const SAMPLES_PER_PERIOD: u32 = 160;
const MEASURED_PERIODS: u32 = 6;
const SETTLING_TIME_CONSTANTS: f64 = 20.0;

fn main() -> Result<(), Box<dyn Error>> {
    let (order, output_path) = arguments()?;
    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    match order {
        1 => plot::<1>(&output_path)?,
        2 => plot::<2>(&output_path)?,
        3 => plot::<3>(&output_path)?,
        4 => plot::<4>(&output_path)?,
        5 => plot::<5>(&output_path)?,
        6 => plot::<6>(&output_path)?,
        7 => plot::<7>(&output_path)?,
        8 => plot::<8>(&output_path)?,
        _ => return Err(format!("order must be between 1 and 8, got {order}").into()),
    }

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
    let responses = [
        (
            "Repeated pole",
            Response::RepeatedPole,
            RGBColor(46, 111, 214),
        ),
        ("Butterworth", Response::Butterworth, RGBColor(224, 91, 74)),
        (
            "Chebyshev I, 0.5 dB ripple",
            Response::Chebyshev1 { ripple_db: 0.5 },
            RGBColor(36, 157, 92),
        ),
    ];

    let root = SVGBackend::new(output_path, (1_200, 760)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("ssfilt order {N} low-pass responses"),
            ("sans-serif", 30),
        )
        .margin(24)
        .x_label_area_size(54)
        .y_label_area_size(70)
        .build_cartesian_2d((0.05_f64..20.0_f64).log_scale(), -100.0_f64..5.0_f64)?;

    chart
        .configure_mesh()
        .x_desc("frequency / configured −3 dB cutoff")
        .y_desc("gain relative to DC (dB)")
        .x_labels(10)
        .y_labels(12)
        .light_line_style(RGBColor(225, 229, 235))
        .draw()?;

    chart.draw_series(LineSeries::new(
        [(0.05, -3.0), (20.0, -3.0)],
        &BLACK.mix(0.25),
    ))?;
    chart.draw_series(LineSeries::new(
        [(1.0, -100.0), (1.0, 5.0)],
        &BLACK.mix(0.25),
    ))?;

    for (label, response, color) in responses {
        println!("measuring {label}");
        let curve = frequency_ratios()
            .map(|ratio| measure_gain_db::<N>(response, ratio).map(|gain| (ratio, gain)))
            .collect::<Result<Vec<_>, _>>()?;
        chart
            .draw_series(LineSeries::new(curve, color.stroke_width(3)))?
            .label(label)
            .legend(move |(x, y)| PathElement::new([(x, y), (x + 28, y)], color.stroke_width(3)));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.9))
        .border_style(BLACK.mix(0.35))
        .position(SeriesLabelPosition::LowerLeft)
        .draw()?;
    root.present()?;
    Ok(())
}

fn frequency_ratios() -> impl Iterator<Item = f64> {
    let start = 0.05_f64.log10();
    let end = 20.0_f64.log10();
    (0..POINTS).map(move |index| {
        let fraction = f64::from(index) / f64::from(POINTS - 1);
        10.0_f64.powf(start + fraction * (end - start))
    })
}

fn measure_gain_db<const N: usize>(
    response: Response,
    frequency_ratio: f64,
) -> Result<f64, Box<dyn Error>> {
    let frequency_hz = CUTOFF_HZ * frequency_ratio;
    let dt = 1.0 / (frequency_hz * f64::from(SAMPLES_PER_PERIOD));
    let settling_samples = (SETTLING_TIME_CONSTANTS / CUTOFF_HZ / dt)
        .ceil()
        .to_u32()
        .ok_or("settling sample count does not fit in u32")?;
    let measured_samples = MEASURED_PERIODS * SAMPLES_PER_PERIOD;
    let mut filter = LowPass::<N>::builder(CUTOFF_HZ)
        .response(response)
        .input_model(InputModel::Linear)
        .build()?;

    for index in 1..=settling_samples {
        let phase = core::f64::consts::TAU * frequency_hz * f64::from(index) * dt;
        filter.update(phase.sin(), dt)?;
    }

    let mut in_phase = 0.0;
    let mut quadrature = 0.0;
    for offset in 1..=measured_samples {
        let index = settling_samples + offset;
        let phase = core::f64::consts::TAU * frequency_hz * f64::from(index) * dt;
        let output = filter.update(phase.sin(), dt)?;
        in_phase += output * phase.sin();
        quadrature += output * phase.cos();
    }

    let gain = 2.0 * in_phase.hypot(quadrature) / f64::from(measured_samples);
    Ok(20.0 * gain.max(1.0e-12).log10())
}
