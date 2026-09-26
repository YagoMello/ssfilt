//! A four-tone burst makes frequency-dependent delay visible in the time domain.

use std::error::Error;
use std::path::Path;

use num_traits::ToPrimitive;
use plotters::coord::Shift;
use plotters::prelude::*;
use ssfilt::{InputModel, LowPass, MAX_BESSEL_ORDER, PhaseEqualizedLowPass, Response};

use super::{
    CUTOFF_HZ, DISPLAY_HEIGHT, DISPLAY_WIDTH, PhaseModel, RENDER_SCALE, WAVEFORM_SAMPLES_PER_CYCLE,
    WAVEFORM_TONES, exact_frequency_point, phase_equalizer, scaled, smooth_svg_response_curves,
};

const BURST_CENTER: f64 = 8.0;
const BURST_WIDTH: f64 = 1.8;
const VIEW_START: f64 = -3.0;
const ALIGNED_END: f64 = 3.5;
const PHASE_QUADRATURE_STEPS: u32 = 512;
const INPUT_COLOR: RGBColor = RGBColor(112, 120, 130);
const PLAIN_COLOR: RGBColor = RGBColor(46, 111, 214);
const EQUALIZED_COLOR: RGBColor = RGBColor(224, 91, 74);

struct Trace {
    dt: f64,
    input: Vec<f64>,
    plain: Vec<f64>,
    equalized: Option<Vec<f64>>,
    plain_delay: f64,
    equalized_delay: Option<f64>,
}

pub(super) fn plot_waveform_comparison<const N: usize>(
    model: PhaseModel,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if model == PhaseModel::Bessel && N > MAX_BESSEL_ORDER {
        return Err(format!("Bessel is supported through order {MAX_BESSEL_ORDER}").into());
    }
    let trace = measure_trace::<N>(model)?;
    let max_delay = trace
        .equalized_delay
        .unwrap_or(trace.plain_delay)
        .max(trace.plain_delay);
    let raw_end = max_delay + 3.5;
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!(
            "ssfilt order {N} {}: {}",
            model.label(),
            if trace.equalized.is_some() {
                "coherent four-tone burst (0.12–0.88 fc)"
            } else {
                "four-tone burst; no improving all-pass design"
            }
        ),
        ("sans-serif", scaled(30)),
    )?;
    let panels = root.split_evenly((2, 1));
    draw_panel(
        &panels[0],
        &trace,
        VIEW_START..raw_end,
        false,
        if trace.equalized.is_some() {
            "As streamed: all-pass adds real latency"
        } else {
            "As streamed: ordinary filter"
        },
    )?;
    draw_panel(
        &panels[1],
        &trace,
        VIEW_START..ALIGNED_END,
        true,
        "Aligned by one fitted constant delay per output (display only)",
    )?;
    root.present()?;
    drop(panels);
    drop(root);
    smooth_svg_response_curves(output_path)?;
    if trace.equalized.is_none() {
        println!(
            "no improving all-pass design for order {N} {}",
            model.label()
        );
    }
    Ok(())
}

fn measure_trace<const N: usize>(model: PhaseModel) -> Result<Trace, Box<dyn Error>> {
    let mut plain = LowPass::<N>::builder(CUTOFF_HZ)
        .response(model.response())
        .input_model(InputModel::Linear)
        .build()?;
    let mut equalized = phase_equalizer(&plain)?;
    let plain_delay = fitted_delay::<N>(model.response(), None)?;
    let equalized_delay = equalized
        .as_ref()
        .map(|filter| fitted_delay::<N>(model.response(), Some(filter)))
        .transpose()?;
    let max_delay = equalized_delay.unwrap_or(plain_delay).max(plain_delay);
    let dt = 1.0 / (f64::from(WAVEFORM_SAMPLES_PER_CYCLE) * CUTOFF_HZ);
    let duration = BURST_CENTER + max_delay + 6.0;
    let count = (duration / dt)
        .ceil()
        .to_usize()
        .ok_or("invalid waveform duration")?;
    let mut trace = Trace {
        dt,
        input: Vec::with_capacity(count + 1),
        plain: Vec::with_capacity(count + 1),
        equalized: equalized.as_ref().map(|_| Vec::with_capacity(count + 1)),
        plain_delay,
        equalized_delay,
    };
    trace.input.push(burst(0.0));
    trace.plain.push(plain.output());
    if let Some(points) = trace.equalized.as_mut() {
        points.push(equalized.as_ref().ok_or("missing equalizer")?.output());
    }
    for index in 1..=count {
        let time = f64::from(u32::try_from(index)?) * dt * CUTOFF_HZ;
        let input = burst(time);
        trace.input.push(input);
        trace.plain.push(plain.update(input, dt)?);
        if let Some(points) = trace.equalized.as_mut() {
            points.push(
                equalized
                    .as_mut()
                    .ok_or("missing equalizer")?
                    .update(input, dt)?,
            );
        }
    }
    Ok(trace)
}

fn burst(time: f64) -> f64 {
    let relative = time - BURST_CENTER;
    let envelope = (-0.5 * (relative / BURST_WIDTH).powi(2)).exp();
    let coherent = WAVEFORM_TONES
        .iter()
        .map(|frequency| (std::f64::consts::TAU * frequency * relative).cos())
        .sum::<f64>()
        / f64::from(u32::try_from(WAVEFORM_TONES.len()).expect("small tone count"));
    envelope * coherent
}

fn fitted_delay<const N: usize>(
    response: Response,
    equalized: Option<&PhaseEqualizedLowPass<N, 2>>,
) -> Result<f64, Box<dyn Error>> {
    let mut phase_weighted = 0.0;
    let mut frequency_squared = 0.0;
    for ratio in WAVEFORM_TONES {
        let angular = std::f64::consts::TAU * ratio;
        let phase = if let Some(filter) = equalized {
            equalized_phase(filter, ratio)?
        } else {
            exact_frequency_point::<N>(response, ratio)?.phase_radians
        };
        phase_weighted += angular * phase;
        frequency_squared += angular * angular;
    }
    Ok(-phase_weighted / frequency_squared)
}

fn equalized_phase<const N: usize>(
    filter: &PhaseEqualizedLowPass<N, 2>,
    ratio: f64,
) -> Result<f64, Box<dyn Error>> {
    let interval = ratio / f64::from(PHASE_QUADRATURE_STEPS);
    let mut weighted_delay = 0.0;
    for index in 0..=PHASE_QUADRATURE_STEPS {
        let weight = if index == 0 || index == PHASE_QUADRATURE_STEPS {
            1.0
        } else if index % 2 == 0 {
            2.0
        } else {
            4.0
        };
        weighted_delay += weight
            * filter.group_delay_seconds(f64::from(index) * interval * CUTOFF_HZ)?
            * std::f64::consts::TAU;
    }
    Ok(-weighted_delay * interval / 3.0)
}

fn draw_panel(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    trace: &Trace,
    x_range: std::ops::Range<f64>,
    aligned: bool,
    title: &str,
) -> Result<(), Box<dyn Error>> {
    let input = points(&trace.input, trace.dt, x_range.clone(), 0.0);
    let plain = points(
        &trace.plain,
        trace.dt,
        x_range.clone(),
        if aligned { trace.plain_delay } else { 0.0 },
    );
    let equalized = trace.equalized.as_ref().map(|samples| {
        points(
            samples,
            trace.dt,
            x_range.clone(),
            if aligned {
                trace.equalized_delay.unwrap_or(0.0)
            } else {
                0.0
            },
        )
    });
    let peak = input
        .iter()
        .chain(&plain)
        .chain(equalized.iter().flat_map(|points| points.iter()))
        .map(|(_, value)| value.abs())
        .fold(1.0_f64, f64::max)
        * 1.12;
    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(48))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(x_range, -peak..peak)?;
    chart
        .configure_mesh()
        .x_desc("time relative to burst center × cutoff frequency")
        .y_desc("amplitude")
        .x_labels(12)
        .y_labels(7)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart
        .draw_series(LineSeries::new(input, INPUT_COLOR.stroke_width(scaled(2))))?
        .label("Input")
        .legend(|(x, y)| PathElement::new([(x, y), (x + 24, y)], INPUT_COLOR.stroke_width(2)));
    chart
        .draw_series(LineSeries::new(plain, PLAIN_COLOR.stroke_width(scaled(2))))?
        .label(format!("Plain (fit {:.2} cycles)", trace.plain_delay))
        .legend(|(x, y)| PathElement::new([(x, y), (x + 24, y)], PLAIN_COLOR.stroke_width(2)));
    if let Some(equalized) = equalized {
        chart
            .draw_series(LineSeries::new(
                equalized,
                EQUALIZED_COLOR.stroke_width(scaled(2)),
            ))?
            .label(format!(
                "Equalized (fit {:.2} cycles)",
                trace.equalized_delay.unwrap_or(0.0)
            ))
            .legend(|(x, y)| {
                PathElement::new([(x, y), (x + 24, y)], EQUALIZED_COLOR.stroke_width(2))
            });
    }
    chart
        .configure_series_labels()
        .label_font(("sans-serif", scaled(12)))
        .margin(scaled(5))
        .legend_area_size(scaled(30))
        .background_style(WHITE)
        .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
        .position(SeriesLabelPosition::UpperRight)
        .draw()?;
    Ok(())
}

fn points(samples: &[f64], dt: f64, range: std::ops::Range<f64>, advance: f64) -> Vec<(f64, f64)> {
    (0..samples.len())
        .filter_map(|index| {
            let time = f64::from(u32::try_from(index).ok()?) * dt * CUTOFF_HZ;
            let relative = time - BURST_CENTER;
            if relative < range.start || relative > range.end {
                return None;
            }
            let value = interpolate(samples, (time + advance) / (dt * CUTOFF_HZ))?;
            Some((relative, value))
        })
        .collect()
}

fn interpolate(samples: &[f64], index: f64) -> Option<f64> {
    let left = index.floor().to_usize()?;
    let first = *samples.get(left)?;
    let second = *samples.get(left + 1).unwrap_or(&first);
    Some((second - first).mul_add(index - f64::from(u32::try_from(left).ok()?), first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn burst_is_coherent_at_center_and_small_at_start() {
        assert_relative_eq!(burst(BURST_CENTER), 1.0, epsilon = 1.0e-14);
        assert!(burst(0.0).abs() < 0.001);
    }

    #[test]
    fn equalized_delay_fit_exceeds_plain_butterworth() {
        let plain = LowPass::<4>::builder(CUTOFF_HZ)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let equalized = phase_equalizer(&plain).unwrap().unwrap();
        let before = fitted_delay::<4>(Response::Butterworth, None).unwrap();
        let after = fitted_delay::<4>(Response::Butterworth, Some(&equalized)).unwrap();
        assert!(after > before);
        let plain_error = WAVEFORM_TONES
            .iter()
            .map(|&ratio| {
                let phase = exact_frequency_point::<4>(Response::Butterworth, ratio)
                    .unwrap()
                    .phase_radians;
                (phase + std::f64::consts::TAU * ratio * before).powi(2)
            })
            .sum::<f64>();
        let equalized_error = WAVEFORM_TONES
            .iter()
            .map(|&ratio| {
                let phase = equalized_phase(&equalized, ratio).unwrap();
                (phase + std::f64::consts::TAU * ratio * after).powi(2)
            })
            .sum::<f64>();
        assert!(equalized_error < plain_error);
    }

    #[test]
    fn interpolation_at_integer_and_half_samples() {
        let samples = [0.0, 2.0, 4.0];
        assert_relative_eq!(interpolate(&samples, 1.0).unwrap(), 2.0);
        assert_relative_eq!(interpolate(&samples, 1.5).unwrap(), 3.0);
    }
}
