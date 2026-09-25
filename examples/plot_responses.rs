//! Generates end-to-end response plots through the public streaming API.
//!
//! Usage:
//! `cargo run --release --example plot_responses -- [order] [output.svg]`

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use num_traits::ToPrimitive;
use plotters::coord::Shift;
use plotters::prelude::*;
use ssfilt::{InputModel, LowPass, Response};

const CUTOFF_HZ: f64 = 1.0;
const FREQUENCY_POINTS: u32 = 1_601;
const TRANSIENT_POINTS: u32 = 1_601;
const SAMPLES_PER_PERIOD: u32 = 160;
const MEASURED_PERIODS: u32 = 6;
const SETTLING_TIME_CONSTANTS: f64 = 20.0;
const MIN_FREQUENCY_RATIO: f64 = 0.01;
const MAX_FREQUENCY_RATIO: f64 = 20.0;
const MAGNITUDE_FLOOR_DB: f64 = -140.0;
const PHASE_GAIN_FLOOR_DB: f64 = -80.0;
const GROUP_DELAY_GAIN_FLOOR_DB: f64 = -50.0;
const GROUP_DELAY_HALF_WINDOW: usize = 4;
const RESPONSE_STROKES: [&str; 3] = ["#2E6FD6", "#E05B4A", "#249D5C"];

#[derive(Clone, Copy)]
struct ResponseSpec {
    label: &'static str,
    response: Response,
    color: RGBColor,
}

struct FrequencyPoint {
    ratio: f64,
    gain_db: f64,
    phase_radians: f64,
    reliable: bool,
}

struct ResponseData {
    spec: ResponseSpec,
    frequency: Vec<FrequencyPoint>,
    group_delay: Vec<(f64, f64)>,
    step: Vec<(f64, f64)>,
}

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
    let specs = [
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
            label: "Chebyshev I, 0.5 dB ripple",
            response: Response::Chebyshev1 { ripple_db: 0.5 },
            color: RGBColor(36, 157, 92),
        },
    ];

    let mut responses = Vec::with_capacity(specs.len());
    for spec in specs {
        println!("measuring {}", spec.label);
        let mut frequency = frequency_ratios()
            .map(|ratio| measure_frequency_point::<N>(spec.response, ratio))
            .collect::<Result<Vec<_>, _>>()?;
        mark_stopband_measurement_limit(&mut frequency);
        unwrap_phase(&mut frequency);
        let group_delay = group_delay(&frequency);
        let step = measure_step_response::<N>(spec.response)?;
        responses.push(ResponseData {
            spec,
            frequency,
            group_delay,
            step,
        });
    }

    let root = SVGBackend::new(output_path, (1_600, 1_180)).into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!("ssfilt order {N} low-pass responses"),
        ("sans-serif", 30),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_magnitude(&panels[0], &responses)?;
    draw_phase::<N>(&panels[1], &responses)?;
    draw_group_delay(&panels[2], &responses)?;
    draw_step(&panels[3], &responses)?;
    root.present()?;
    drop(panels);
    drop(root);
    smooth_svg_response_curves(output_path)?;
    Ok(())
}

fn draw_magnitude(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let mut chart = ChartBuilder::on(area)
        .caption("Magnitude response", ("sans-serif", 22))
        .margin(16)
        .x_label_area_size(46)
        .y_label_area_size(62)
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            MAGNITUDE_FLOOR_DB..5.0_f64,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency / −3 dB cutoff")
        .y_desc("gain relative to DC (dB)")
        .x_labels(9)
        .y_labels(10)
        .light_line_style(RGBColor(225, 229, 235))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(MIN_FREQUENCY_RATIO, -3.0), (MAX_FREQUENCY_RATIO, -3.0)],
        &BLACK.mix(0.25),
    ))?;
    chart.draw_series(LineSeries::new(
        [(1.0, MAGNITUDE_FLOOR_DB), (1.0, 5.0)],
        &BLACK.mix(0.25),
    ))?;
    for response in responses {
        chart
            .draw_series(LineSeries::new(
                response
                    .frequency
                    .iter()
                    .filter(|point| point.reliable)
                    .map(|point| (point.ratio, point.gain_db)),
                response.spec.color.stroke_width(2),
            ))?
            .label(response.spec.label)
            .legend(move |(x, y)| {
                PathElement::new([(x, y), (x + 24, y)], response.spec.color.stroke_width(2))
            });
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.9))
        .border_style(BLACK.mix(0.35))
        .position(SeriesLabelPosition::LowerLeft)
        .draw()?;
    Ok(())
}

fn draw_phase<const N: usize>(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let phase_floor = -90.0 * N.to_f64().ok_or("order does not fit in f64")?;
    let mut chart = ChartBuilder::on(area)
        .caption("Unwrapped phase response", ("sans-serif", 22))
        .margin(16)
        .x_label_area_size(46)
        .y_label_area_size(68)
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            phase_floor..5.0_f64,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency / −3 dB cutoff")
        .y_desc("phase (degrees)")
        .x_labels(9)
        .y_labels(10)
        .light_line_style(RGBColor(225, 229, 235))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(1.0, phase_floor), (1.0, 5.0)],
        &BLACK.mix(0.25),
    ))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response
                .frequency
                .iter()
                .filter(|point| point.reliable && point.gain_db >= PHASE_GAIN_FLOOR_DB)
                .map(|point| (point.ratio, point.phase_radians.to_degrees())),
            response.spec.color.stroke_width(2),
        ))?;
    }
    Ok(())
}

fn draw_group_delay(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let maximum = responses
        .iter()
        .flat_map(|response| response.group_delay.iter().map(|(_, delay)| *delay))
        .filter(|delay| delay.is_finite() && *delay >= 0.0)
        .fold(1.0_f64, f64::max);
    let upper = maximum.mul_add(1.05, 0.05);
    let mut chart = ChartBuilder::on(area)
        .caption("Group delay", ("sans-serif", 22))
        .margin(16)
        .x_label_area_size(46)
        .y_label_area_size(68)
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            0.0_f64..upper,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency / −3 dB cutoff")
        .y_desc("normalized group delay (ωc τg)")
        .x_labels(9)
        .y_labels(10)
        .light_line_style(RGBColor(225, 229, 235))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(1.0, 0.0), (1.0, upper)],
        &BLACK.mix(0.25),
    ))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response.group_delay.iter().copied(),
            response.spec.color.stroke_width(2),
        ))?;
    }
    Ok(())
}

fn draw_step(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let end = responses
        .first()
        .and_then(|response| response.step.last())
        .map_or(4.0, |(time, _)| *time);
    let maximum = responses
        .iter()
        .flat_map(|response| response.step.iter().map(|(_, value)| *value))
        .filter(|value| value.is_finite())
        .fold(1.0_f64, f64::max);
    let mut chart = ChartBuilder::on(area)
        .caption("Unit-step response", ("sans-serif", 22))
        .margin(16)
        .x_label_area_size(46)
        .y_label_area_size(62)
        .build_cartesian_2d(0.0_f64..end, -0.1_f64..maximum.mul_add(1.05, 0.05))?;
    chart
        .configure_mesh()
        .x_desc("time × cutoff frequency")
        .y_desc("output")
        .x_labels(9)
        .y_labels(10)
        .light_line_style(RGBColor(225, 229, 235))
        .draw()?;
    chart.draw_series(LineSeries::new([(0.0, 1.0), (end, 1.0)], &BLACK.mix(0.25)))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response.step.iter().copied(),
            response.spec.color.stroke_width(2),
        ))?;
    }
    Ok(())
}

fn frequency_ratios() -> impl Iterator<Item = f64> {
    let start = MIN_FREQUENCY_RATIO.log10();
    let end = MAX_FREQUENCY_RATIO.log10();
    (0..FREQUENCY_POINTS).map(move |index| {
        let fraction = f64::from(index) / f64::from(FREQUENCY_POINTS - 1);
        10.0_f64.powf(start + fraction * (end - start))
    })
}

fn measure_frequency_point<const N: usize>(
    response: Response,
    frequency_ratio: f64,
) -> Result<FrequencyPoint, Box<dyn Error>> {
    let frequency_hz = CUTOFF_HZ * frequency_ratio;
    let dt = 1.0 / (frequency_hz * f64::from(SAMPLES_PER_PERIOD));
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let settling_duration = SETTLING_TIME_CONSTANTS.max(order) / CUTOFF_HZ;
    let settling_samples = (settling_duration / dt)
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

    let scale = 2.0 / f64::from(measured_samples);
    let real = scale * in_phase;
    let imaginary = scale * quadrature;
    let gain = real.hypot(imaginary);
    Ok(FrequencyPoint {
        ratio: frequency_ratio,
        gain_db: (20.0 * gain.max(f64::MIN_POSITIVE).log10()).max(MAGNITUDE_FLOOR_DB),
        phase_radians: imaginary.atan2(real),
        reliable: true,
    })
}

fn unwrap_phase(points: &mut [FrequencyPoint]) {
    let Some((first, remaining)) = points.split_first_mut() else {
        return;
    };
    let mut previous = first.phase_radians;
    for point in remaining {
        while point.phase_radians - previous > core::f64::consts::PI {
            point.phase_radians -= core::f64::consts::TAU;
        }
        while point.phase_radians - previous < -core::f64::consts::PI {
            point.phase_radians += core::f64::consts::TAU;
        }
        previous = point.phase_radians;
    }
}

fn mark_stopband_measurement_limit(points: &mut [FrequencyPoint]) {
    let mut preceding_gain = f64::INFINITY;
    let mut reliable = true;
    for point in points.iter_mut().filter(|point| point.ratio >= 1.0) {
        if point.gain_db > preceding_gain + 0.25 {
            reliable = false;
        }
        point.reliable = reliable;
        preceding_gain = preceding_gain.min(point.gain_db);
    }
}

fn group_delay(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let width = 2 * GROUP_DELAY_HALF_WINDOW + 1;
    points
        .windows(width)
        .filter_map(|window| {
            if window
                .iter()
                .any(|point| !point.reliable || point.gain_db < GROUP_DELAY_GAIN_FLOOR_DB)
            {
                return None;
            }
            let center = &window[GROUP_DELAY_HALF_WINDOW];
            let mean_ratio = window.iter().map(|point| point.ratio).sum::<f64>()
                / width.to_f64().expect("small constant fits in f64");
            let mean_phase = window.iter().map(|point| point.phase_radians).sum::<f64>()
                / width.to_f64().expect("small constant fits in f64");
            let (covariance, variance) = window.iter().fold((0.0, 0.0), |acc, point| {
                let ratio_offset = point.ratio - mean_ratio;
                (
                    (point.phase_radians - mean_phase).mul_add(ratio_offset, acc.0),
                    ratio_offset.mul_add(ratio_offset, acc.1),
                )
            });
            let delay = -covariance / variance;
            (delay.is_finite() && delay >= 0.0).then_some((center.ratio, delay))
        })
        .collect()
}

fn measure_step_response<const N: usize>(
    response: Response,
) -> Result<Vec<(f64, f64)>, Box<dyn Error>> {
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let duration = 4.0_f64.max(order);
    let dt = duration / (CUTOFF_HZ * f64::from(TRANSIENT_POINTS - 1));
    let mut filter = LowPass::<N>::builder(CUTOFF_HZ)
        .response(response)
        .input_model(InputModel::CurrentHold)
        .build()?;
    let mut points = Vec::with_capacity(TRANSIENT_POINTS as usize);
    points.push((0.0, filter.output()));
    for index in 1..TRANSIENT_POINTS {
        let output = filter.update(1.0, dt)?;
        points.push((f64::from(index) * dt * CUTOFF_HZ, output));
    }
    Ok(points)
}

fn smooth_svg_response_curves(output_path: &Path) -> Result<(), Box<dyn Error>> {
    // Plotters maps chart coordinates to integer backend pixels and emits
    // polylines. Replacing only the response curves with shape-preserving
    // cubic paths removes visible quantization at high SVG zoom levels.
    let svg = fs::read_to_string(output_path)?;
    let mut smoothed = String::with_capacity(svg.len());
    for line in svg.lines() {
        let replacement = RESPONSE_STROKES
            .iter()
            .any(|color| line.contains(color))
            .then(|| smooth_polyline(line))
            .flatten();
        smoothed.push_str(replacement.as_deref().unwrap_or(line));
        smoothed.push('\n');
    }
    fs::write(output_path, smoothed)?;
    Ok(())
}

fn smooth_polyline(line: &str) -> Option<String> {
    let points_start = line.find(" points=\"")?;
    let values_start = points_start + " points=\"".len();
    let values_end = values_start + line[values_start..].find('"')?;
    let points = collapse_vertical_pixels(parse_svg_points(&line[values_start..values_end])?);
    if points.len() < 3 {
        return None;
    }

    let prefix = line[..points_start].replacen("<polyline", "<path", 1);
    let suffix = &line[values_end + 1..];
    Some(format!(
        "{prefix} stroke-linecap=\"round\" stroke-linejoin=\"round\" d=\"{}\"{suffix}",
        pchip_path(&points)?
    ))
}

fn parse_svg_points(values: &str) -> Option<Vec<(i32, i32)>> {
    values
        .split_ascii_whitespace()
        .map(|point| {
            let (x, y) = point.split_once(',')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

fn collapse_vertical_pixels(points: Vec<(i32, i32)>) -> Vec<(f64, f64)> {
    let mut collapsed: Vec<(i32, f64, u32)> = Vec::with_capacity(points.len());
    for (x, y) in points {
        if let Some((last_x, y_sum, count)) = collapsed.last_mut() {
            if *last_x == x {
                *y_sum += f64::from(y);
                *count += 1;
                continue;
            }
        }
        collapsed.push((x, f64::from(y), 1));
    }
    collapsed
        .into_iter()
        .map(|(x, y_sum, count)| (f64::from(x), y_sum / f64::from(count)))
        .collect()
}

fn pchip_path(points: &[(f64, f64)]) -> Option<String> {
    let intervals = points
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect::<Vec<_>>();
    if intervals.iter().any(|interval| *interval <= 0.0) {
        return None;
    }
    let secants = points
        .windows(2)
        .zip(&intervals)
        .map(|(pair, interval)| (pair[1].1 - pair[0].1) / interval)
        .collect::<Vec<_>>();
    let slopes = pchip_slopes(&intervals, &secants);

    let mut path = String::with_capacity(points.len() * 64);
    write!(path, "M {:.3},{:.3}", points[0].0, points[0].1).ok()?;
    for (index, pair) in points.windows(2).enumerate() {
        let interval = intervals[index];
        let control_1 = (
            pair[0].0 + interval / 3.0,
            slopes[index].mul_add(interval / 3.0, pair[0].1),
        );
        let control_2 = (
            pair[1].0 - interval / 3.0,
            (-slopes[index + 1]).mul_add(interval / 3.0, pair[1].1),
        );
        write!(
            path,
            " C {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
            control_1.0, control_1.1, control_2.0, control_2.1, pair[1].0, pair[1].1
        )
        .ok()?;
    }
    Some(path)
}

fn pchip_slopes(intervals: &[f64], secants: &[f64]) -> Vec<f64> {
    if secants.len() == 1 {
        return vec![secants[0], secants[0]];
    }

    let mut slopes = vec![0.0; secants.len() + 1];
    slopes[0] = endpoint_slope(intervals[0], intervals[1], secants[0], secants[1]);
    let last = secants.len() - 1;
    slopes[last + 1] = endpoint_slope(
        intervals[last],
        intervals[last - 1],
        secants[last],
        secants[last - 1],
    );
    for index in 1..=last {
        let preceding = secants[index - 1];
        let following = secants[index];
        if preceding * following <= 0.0 {
            continue;
        }
        let weight_1 = 2.0 * intervals[index] + intervals[index - 1];
        let weight_2 = intervals[index] + 2.0 * intervals[index - 1];
        slopes[index] = (weight_1 + weight_2) / (weight_1 / preceding + weight_2 / following);
    }
    slopes
}

fn endpoint_slope(
    endpoint_interval: f64,
    adjacent_interval: f64,
    endpoint_secant: f64,
    adjacent_secant: f64,
) -> f64 {
    let mut slope = ((2.0 * endpoint_interval + adjacent_interval) * endpoint_secant
        - endpoint_interval * adjacent_secant)
        / (endpoint_interval + adjacent_interval);
    if slope * endpoint_secant <= 0.0 {
        slope = 0.0;
    } else if endpoint_secant * adjacent_secant < 0.0 && slope.abs() > 3.0 * endpoint_secant.abs() {
        slope = 3.0 * endpoint_secant;
    }
    slope
}

#[cfg(test)]
mod svg_tests {
    use super::{collapse_vertical_pixels, pchip_path, smooth_polyline};

    #[test]
    fn vertical_pixel_runs_are_averaged() {
        let points = collapse_vertical_pixels(vec![(1, 2), (1, 4), (2, 5)]);
        assert_eq!(points, vec![(1.0, 3.0), (2.0, 5.0)]);
    }

    #[test]
    fn pchip_emits_cubic_segments_through_each_point() {
        let path = pchip_path(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]).unwrap();
        assert!(path.starts_with("M 0.000,0.000 C "));
        assert!(path.contains(" 1.000,1.000 C "));
        assert!(path.ends_with(" 2.000,0.000"));
    }

    #[test]
    fn response_polyline_becomes_a_rounded_path() {
        let line = concat!(
            "<polyline fill=\"none\" stroke=\"#2E6FD6\" ",
            "points=\"0,0 1,1 2,0 \"/>"
        );
        let path = smooth_polyline(line).unwrap();
        assert!(path.starts_with("<path "));
        assert!(path.contains("stroke-linecap=\"round\""));
        assert!(path.contains(" d=\"M "));
        assert!(!path.contains(" points="));
    }
}
