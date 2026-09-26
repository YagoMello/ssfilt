//! Indicative, host-local streaming timing chart for development.
//!
//! Usage: `cargo run --release --example plot_performance -- [output.svg]`

use std::env;
use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use num_traits::ToPrimitive;
use plotters::coord::Shift;
use plotters::prelude::*;
use ssfilt::{
    BandPass, HighPass, InputModel, IntegrationConfig, LowPass, Response, StreamingFilter,
    Tolerances,
};

const CUTOFF_HZ: f64 = 1_000.0;
const DT_SECONDS: f64 = 1.0 / (8.0 * CUTOFF_HZ);
const SAMPLES: usize = 2_048;
const TRIALS: usize = 5;
const WIDTH: u32 = 1_600;
const HEIGHT: u32 = 840;

struct Timing {
    label: &'static str,
    ns_per_sample: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let output = arguments.next().map_or_else(
        || PathBuf::from("target/filter-performance.svg"),
        PathBuf::from,
    );
    if let Some(extra) = arguments.next() {
        return Err(format!("unexpected argument: {extra}").into());
    }
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    let kernel_cases = kernel_cases()?;
    let integrator_cases = integrator_cases()?;

    let root = SVGBackend::new(&output, (WIDTH, HEIGHT)).into_drawing_area();
    root.fill(&WHITE)?;
    root.draw(&Text::new(
        "ssfilt streaming cost",
        (48, 60),
        ("sans-serif", 36).into_font(),
    ))?;
    root.draw(&Text::new(
        "Median of five release-mode trials; 2,048 updates/trial; order 4; f = 1 kHz; Δt = 125 µs",
        (48, 92),
        ("sans-serif", 18)
            .into_font()
            .color(&RGBColor(90, 100, 110)),
    ))?;
    let panels = root.margin(120, 100, 45, 45).split_evenly((1, 2));
    draw_panel(
        &panels[0],
        "Kernel, topology & all-pass",
        &kernel_cases,
        RGBColor(46, 111, 214),
    )?;
    draw_panel(
        &panels[1],
        "RK45 settings & input model",
        &integrator_cases,
        RGBColor(36, 157, 92),
    )?;
    root.draw(&Text::new(
        "Lower is faster. Host-local indicative timings, not a portable benchmark; only RK45 is currently implemented.",
        (48, 804), ("sans-serif", 17).into_font().color(&RGBColor(90, 100, 110)),
    ))?;
    root.present()?;
    drop(panels);
    drop(root);
    println!("wrote {}", output.display());
    Ok(())
}

fn kernel_cases() -> Result<Vec<Timing>, Box<dyn Error>> {
    let base = LowPass::<4>::builder(CUTOFF_HZ)
        .response(Response::Butterworth)
        .build()?;
    let kernel_cases = vec![
        timing(
            "Repeated pole LP",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::RepeatedPole)
                .build()?,
        )?,
        timing("Butterworth LP", base)?,
        timing(
            "Bessel LP",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Bessel)
                .build()?,
        )?,
        timing(
            "Chebyshev I LP",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Chebyshev1 { ripple_db: 0.5 })
                .build()?,
        )?,
        timing(
            "Butterworth HP",
            HighPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .build()?,
        )?,
        timing(
            "Butterworth BP",
            BandPass::<4>::builder(CUTOFF_HZ / 2.0, CUTOFF_HZ * 2.0)
                .response(Response::Butterworth)
                .build()?,
        )?,
        timing(
            "Butter LP + AP(2)",
            base.equalize_phase::<2>(0.0, CUTOFF_HZ)?,
        )?,
    ];
    Ok(kernel_cases)
}

fn integrator_cases() -> Result<Vec<Timing>, Box<dyn Error>> {
    let base = LowPass::<4>::builder(CUTOFF_HZ)
        .response(Response::Butterworth)
        .build()?;
    let loose = IntegrationConfig::<f64> {
        tolerances: Tolerances::new(1.0e-7, 1.0e-4),
        ..Default::default()
    };
    let tight = IntegrationConfig::<f64> {
        tolerances: Tolerances::new(1.0e-12, 1.0e-9),
        ..Default::default()
    };
    let capped = IntegrationConfig::<f64> {
        max_step_seconds: Some(DT_SECONDS / 8.0),
        ..Default::default()
    };
    let integrator_cases = vec![
        timing("RK45 default / linear", base)?,
        timing(
            "RK45 loose / linear",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .integration(loose)
                .build()?,
        )?,
        timing(
            "RK45 tight / linear",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .integration(tight)
                .build()?,
        )?,
        timing(
            "RK45 capped / linear",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .integration(capped)
                .build()?,
        )?,
        timing(
            "RK45 default / previous hold",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .input_model(InputModel::PreviousHold)
                .build()?,
        )?,
        timing(
            "RK45 default / current hold",
            LowPass::<4>::builder(CUTOFF_HZ)
                .response(Response::Butterworth)
                .input_model(InputModel::CurrentHold)
                .build()?,
        )?,
    ];

    Ok(integrator_cases)
}

fn timing<F: StreamingFilter<Scalar = f64> + Copy>(
    label: &'static str,
    initial: F,
) -> Result<Timing, Box<dyn Error>> {
    let inputs = (0..SAMPLES)
        .map(|index| {
            let index = f64::from(u32::try_from(index).expect("small sample index"));
            (index * 0.13).sin() * 0.4 + (index * 0.031).cos() * 0.2
        })
        .collect::<Vec<_>>();
    let mut samples = Vec::with_capacity(TRIALS);
    for trial in 0..=TRIALS {
        let mut filter = initial;
        let start = Instant::now();
        for input in &inputs {
            let input = black_box(*input);
            black_box(filter.update(input, DT_SECONDS)?);
        }
        black_box(filter);
        if trial != 0 {
            samples.push(
                start.elapsed().as_secs_f64() * 1.0e9
                    / f64::from(u32::try_from(SAMPLES).expect("small sample count")),
            );
        }
    }
    samples.sort_by(f64::total_cmp);
    Ok(Timing {
        label,
        ns_per_sample: samples[TRIALS / 2],
    })
}

fn draw_panel(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    title: &str,
    cases: &[Timing],
    color: RGBColor,
) -> Result<(), Box<dyn Error>> {
    let (width, height) = area.dim_in_pixel();
    let width = i32::try_from(width)?;
    let height = i32::try_from(height)?;
    area.draw(&Text::new(
        title.to_owned(),
        (10, 36),
        ("sans-serif", 26).into_font(),
    ))?;
    let left = 290;
    let right = width - 85;
    let top = 90;
    let row_height = (height - top - 95) / i32::try_from(cases.len())?;
    let max = cases
        .iter()
        .map(|case| case.ns_per_sample)
        .fold(0.0_f64, f64::max);
    let axis_max = nice_axis_max(max);
    for tick in 0..=4 {
        let x = left + (right - left) * tick / 4;
        area.draw(&PathElement::new(
            [
                (x, top - 10),
                (x, top + row_height * i32::try_from(cases.len())?),
            ],
            RGBColor(220, 225, 232).stroke_width(1),
        ))?;
        area.draw(&Text::new(
            format!("{:.0}", axis_max * f64::from(tick) / 4.0),
            (x - 14, top + row_height * i32::try_from(cases.len())? + 27),
            ("sans-serif", 14)
                .into_font()
                .color(&RGBColor(90, 100, 110)),
        ))?;
    }
    for (index, case) in cases.iter().enumerate() {
        let y = top + i32::try_from(index)? * row_height;
        let bar_width = (f64::from(right - left) * case.ns_per_sample / axis_max)
            .round()
            .to_i32()
            .ok_or("bar width does not fit in i32")?;
        area.draw(&Text::new(
            case.label,
            (12, y + row_height / 2 + 5),
            ("sans-serif", 16).into_font(),
        ))?;
        area.draw(&Rectangle::new(
            [(left, y + 12), (left + bar_width, y + row_height - 12)],
            color.filled(),
        ))?;
        area.draw(&Text::new(
            format!("{:.0}", case.ns_per_sample),
            (left + bar_width + 5, y + row_height / 2 + 5),
            ("sans-serif", 15).into_font().color(&color),
        ))?;
    }
    area.draw(&Text::new(
        "nanoseconds / update",
        (left, height - 15),
        ("sans-serif", 16).into_font(),
    ))?;
    Ok(())
}

fn nice_axis_max(maximum: f64) -> f64 {
    let magnitude = 10.0_f64.powf(maximum.max(1.0).log10().floor());
    ((maximum * 1.18 / magnitude).ceil() * magnitude).max(magnitude)
}

#[cfg(test)]
mod tests {
    use super::nice_axis_max;

    #[test]
    fn axis_has_headroom() {
        assert!(nice_axis_max(975.0) > 975.0);
        assert!(nice_axis_max(975.0) <= 2_000.0);
    }
}
