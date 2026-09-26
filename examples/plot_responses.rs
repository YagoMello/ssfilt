//! Generates end-to-end response plots through the public streaming API.
//!
//! Usage:
//! `cargo run --release --example plot_responses -- [low-pass|high-pass|band-pass] [order] [output.svg]`
//! `cargo run --release --example plot_responses -- [phase|waveform] [model] [order] [output.svg]`

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use num_traits::ToPrimitive;
use plotters::coord::Shift;
use plotters::prelude::*;
use ssfilt::{
    BandPass, HighPass, InputModel, LowPass, MAX_BESSEL_ORDER, PhaseEqualizationError,
    PhaseEqualizedLowPass, Response,
};

#[path = "../src/model/bessel_table.rs"]
mod bessel_table;

const CUTOFF_HZ: f64 = 1.0;
const FREQUENCY_POINTS: u32 = 1_601;
const TRANSIENT_POINTS: u32 = 1_601;
const MIN_FREQUENCY_RATIO: f64 = 0.01;
const MAX_FREQUENCY_RATIO: f64 = 20.0;
const MAGNITUDE_FLOOR_DB: f64 = -140.0;
const GROUP_DELAY_HALF_WINDOW: usize = 4;
const RESPONSE_STROKES: [&str; 5] = ["#2E6FD6", "#E05B4A", "#249D5C", "#8752A1", "#707882"];
const DISPLAY_WIDTH: u32 = 1_600;
const DISPLAY_HEIGHT: u32 = 1_180;
const RENDER_SCALE: u32 = 8;
const WAVEFORM_TONES: [f64; 4] = [0.12, 0.31, 0.57, 0.88];
const WAVEFORM_SAMPLES_PER_CYCLE: u32 = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    LowPass,
    HighPass,
    BandPass,
    Phase,
    Waveform,
}

impl View {
    fn title(self) -> &'static str {
        match self {
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::BandPass => "band-pass",
            Self::Phase => "phase comparison",
            Self::Waveform => "phase waveform",
        }
    }

    fn frequency_label(self) -> &'static str {
        match self {
            Self::BandPass => "frequency / geometric band center",
            _ => "frequency / −3 dB cutoff",
        }
    }

    fn step_reference(self) -> f64 {
        match self {
            Self::HighPass | Self::BandPass => 0.0,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhaseModel {
    RepeatedPole,
    Butterworth,
    Bessel,
    Chebyshev1,
}

impl PhaseModel {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "repeated" | "repeated-pole" => Ok(Self::RepeatedPole),
            "butterworth" => Ok(Self::Butterworth),
            "bessel" => Ok(Self::Bessel),
            "chebyshev" | "chebyshev1" => Ok(Self::Chebyshev1),
            _ => Err(format!(
                "unknown model '{value}'; choose repeated, butterworth, bessel, or chebyshev"
            )
            .into()),
        }
    }

    fn response(self) -> Response {
        match self {
            Self::RepeatedPole => Response::RepeatedPole,
            Self::Butterworth => Response::Butterworth,
            Self::Bessel => Response::Bessel,
            Self::Chebyshev1 => Response::Chebyshev1 { ripple_db: 0.5 },
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RepeatedPole => "Repeated pole",
            Self::Butterworth => "Butterworth",
            Self::Bessel => "Bessel",
            Self::Chebyshev1 => "Chebyshev I (0.5 dB ripple)",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::RepeatedPole => "repeated",
            Self::Butterworth => "butterworth",
            Self::Bessel => "bessel",
            Self::Chebyshev1 => "chebyshev",
        }
    }
}

const fn scaled(value: u32) -> u32 {
    value * RENDER_SCALE
}

#[path = "plot_responses/waveform.rs"]
mod waveform_plot;

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
}

struct ResponseData {
    spec: ResponseSpec,
    frequency: Vec<FrequencyPoint>,
    group_delay: Vec<(f64, f64)>,
    step: Vec<(f64, f64)>,
}

macro_rules! dispatch_order {
    ($order:expr, $view:expr, $model:expr, $output:expr; $($supported:literal),+ $(,)?) => {
        match $order {
            $($supported => plot::<$supported>($view, $model, $output),)+
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
    let (view, model, order, output_path) = arguments()?;
    if view == View::BandPass && order % 2 != 0 {
        return Err("band-pass order must be positive and even".into());
    }
    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    dispatch_order!(order, view, model, &output_path;
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50
    )?;

    println!("wrote {}", output_path.display());
    Ok(())
}

fn arguments() -> Result<(View, PhaseModel, usize, PathBuf), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let first = arguments.next();
    let (view, model, order) = match first.as_deref() {
        None => (View::LowPass, PhaseModel::Butterworth, 4),
        Some("low-pass") => (
            View::LowPass,
            PhaseModel::Butterworth,
            parse_order(arguments.next())?,
        ),
        Some("high-pass") => (
            View::HighPass,
            PhaseModel::Butterworth,
            parse_order(arguments.next())?,
        ),
        Some("band-pass") => (
            View::BandPass,
            PhaseModel::Butterworth,
            parse_order(arguments.next())?,
        ),
        Some("phase" | "waveform") => {
            let view = if first.as_deref() == Some("phase") {
                View::Phase
            } else {
                View::Waveform
            };
            let next = arguments.next();
            match next.as_deref() {
                None => (view, PhaseModel::Butterworth, 4),
                Some(value) if value.parse::<usize>().is_ok() => {
                    (view, PhaseModel::Butterworth, value.parse()?)
                }
                Some(value) => (
                    view,
                    PhaseModel::parse(value)?,
                    parse_order(arguments.next())?,
                ),
            }
        }
        Some(value) => (View::LowPass, PhaseModel::Butterworth, value.parse()?),
    };
    let output = arguments.next().map_or_else(
        || {
            PathBuf::from(match view {
                View::LowPass => "target/filter-responses.svg".to_owned(),
                View::Phase if model == PhaseModel::Butterworth => {
                    "target/phase-responses.svg".to_owned()
                }
                View::Phase => format!("target/phase-{}-responses.svg", model.slug()),
                View::Waveform => format!("target/phase-{}-waveform.svg", model.slug()),
                _ => format!("target/{}-responses.svg", view.title()),
            })
        },
        PathBuf::from,
    );
    if let Some(argument) = arguments.next() {
        return Err(format!("unexpected argument: {argument}").into());
    }
    Ok((view, model, order, output))
}

fn parse_order(argument: Option<String>) -> Result<usize, Box<dyn Error>> {
    Ok(argument.map_or(Ok(4), |value| value.parse())?)
}

fn plot<const N: usize>(
    view: View,
    model: PhaseModel,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if view == View::Phase {
        return plot_phase_comparison::<N>(model, output_path);
    }
    if view == View::Waveform {
        return waveform_plot::plot_waveform_comparison::<N>(model, output_path);
    }
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
    let bessel_order = if view == View::BandPass { N / 2 } else { N };
    if bessel_order > MAX_BESSEL_ORDER {
        specs.retain(|spec| spec.response != Response::Bessel);
        println!("omitting Bessel: supported through prototype order {MAX_BESSEL_ORDER}");
    }

    let mut responses = Vec::with_capacity(specs.len());
    for spec in specs {
        println!("evaluating {}", spec.label);
        let frequency = frequency_ratios()
            .map(|ratio| topology_frequency_point::<N>(view, spec.response, ratio))
            .collect::<Result<Vec<_>, _>>()?;
        let group_delay = group_delay(&frequency);
        let step = measure_step_response::<N>(view, spec.response)?;
        responses.push(ResponseData {
            spec,
            frequency,
            group_delay,
            step,
        });
    }

    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!("ssfilt order {N} {} responses", view.title()),
        ("sans-serif", scaled(30)),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_magnitude(&panels[0], &responses, view)?;
    draw_phase(&panels[1], &responses, view)?;
    draw_group_delay(&panels[2], &responses, view)?;
    draw_step(&panels[3], &responses, view)?;
    root.present()?;
    drop(panels);
    drop(root);
    smooth_svg_response_curves(output_path)?;
    Ok(())
}

fn phase_equalizer<const N: usize>(
    ordinary: &LowPass<N>,
) -> Result<Option<PhaseEqualizedLowPass<N, 2>>, Box<dyn Error>> {
    match ordinary.equalize_phase::<2>(0.0, CUTOFF_HZ) {
        Ok(equalized) => Ok(Some(equalized)),
        Err(PhaseEqualizationError::NoImprovement) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn plot_phase_comparison<const N: usize>(
    model: PhaseModel,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if model == PhaseModel::Bessel && N > MAX_BESSEL_ORDER {
        return Err(format!("Bessel is supported through order {MAX_BESSEL_ORDER}").into());
    }
    let response = model.response();
    let ordinary = LowPass::<N>::builder(CUTOFF_HZ)
        .response(response)
        .input_model(InputModel::CurrentHold)
        .build()?;
    let mut equalized = phase_equalizer(&ordinary)?;
    if equalized.is_none() {
        println!(
            "no improving all-pass design for order {N} {}",
            model.label()
        );
    }
    let frequency = frequency_ratios_until(1.0)
        .map(|ratio| exact_frequency_point::<N>(response, ratio))
        .collect::<Result<Vec<_>, _>>()?;
    let mut responses = Vec::with_capacity(2);
    responses.push(ResponseData {
        spec: ResponseSpec {
            label: model.label(),
            response,
            color: RGBColor(46, 111, 214),
        },
        group_delay: group_delay(&frequency),
        frequency,
        step: measure_step_response::<N>(View::LowPass, response)?,
    });
    if let Some(ref mut equalized) = equalized {
        let order = N.to_f64().ok_or("order does not fit in f64")?;
        let duration = 4.0_f64.max(order);
        let dt = duration / (CUTOFF_HZ * f64::from(TRANSIENT_POINTS - 1));
        let mut equalized_step = Vec::with_capacity(TRANSIENT_POINTS as usize);
        equalized_step.push((0.0, equalized.output()));
        for index in 1..TRANSIENT_POINTS {
            let output = equalized.update(1.0, dt)?;
            equalized_step.push((f64::from(index) * dt * CUTOFF_HZ, output));
        }

        let mut phase = 0.0;
        let mut previous_ratio = 0.0;
        let mut previous_delay = equalized.group_delay_seconds(0.0)? * std::f64::consts::TAU;
        let mut equalized_frequency = Vec::with_capacity(responses[0].frequency.len());
        let mut equalized_delay = Vec::with_capacity(responses[0].frequency.len());
        for point in &responses[0].frequency {
            let delay =
                equalized.group_delay_seconds(point.ratio * CUTOFF_HZ)? * std::f64::consts::TAU;
            phase -= (previous_delay + delay) * (point.ratio - previous_ratio) / 2.0;
            equalized_frequency.push(FrequencyPoint {
                ratio: point.ratio,
                gain_db: point.gain_db,
                phase_radians: phase,
            });
            equalized_delay.push((point.ratio, delay));
            previous_ratio = point.ratio;
            previous_delay = delay;
        }
        responses.push(ResponseData {
            spec: ResponseSpec {
                label: "With 2 all-pass sections",
                response,
                color: RGBColor(224, 91, 74),
            },
            group_delay: equalized_delay,
            frequency: equalized_frequency,
            step: equalized_step,
        });
    }
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!(
            "ssfilt order {N} {}: {}",
            model.label(),
            if equalized.is_some() {
                "plain vs equalized (0–fc)"
            } else {
                "no improving all-pass design"
            }
        ),
        ("sans-serif", scaled(30)),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_magnitude(&panels[0], &responses, View::Phase)?;
    draw_phase(&panels[1], &responses, View::Phase)?;
    draw_group_delay(&panels[2], &responses, View::Phase)?;
    draw_step(&panels[3], &responses, View::Phase)?;
    root.present()?;
    drop(panels);
    drop(root);
    smooth_svg_response_curves(output_path)?;
    Ok(())
}

fn draw_magnitude(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
    view: View,
) -> Result<(), Box<dyn Error>> {
    let start = responses[0].frequency[0].ratio;
    let end = responses[0].frequency.last().unwrap().ratio;
    let floor = if view == View::Phase {
        -5.0
    } else {
        MAGNITUDE_FLOOR_DB
    };
    let ceiling = if view == View::Phase { 2.0 } else { 5.0 };
    let mut chart = ChartBuilder::on(area)
        .caption(
            if view == View::Phase && responses.len() > 1 {
                "Magnitude response (traces coincide)"
            } else {
                "Magnitude response"
            },
            ("sans-serif", scaled(22)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d((start..end).log_scale(), floor..ceiling)?;
    chart
        .configure_mesh()
        .x_desc(view.frequency_label())
        .y_desc(if view == View::LowPass || view == View::Phase {
            "gain relative to DC (dB)"
        } else {
            "gain relative to passband (dB)"
        })
        .x_labels(9)
        .y_labels(10)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(start, -3.0), (end, -3.0)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    chart.draw_series(LineSeries::new(
        [(1.0, floor), (1.0, ceiling)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    if view == View::BandPass {
        for edge in [0.5, 2.0] {
            chart.draw_series(LineSeries::new(
                [(edge, floor), (edge, ceiling)],
                BLACK.mix(0.25).stroke_width(RENDER_SCALE),
            ))?;
        }
    }
    for response in responses {
        chart
            .draw_series(LineSeries::new(
                visible_magnitude_points(&response.frequency),
                response.spec.color.stroke_width(scaled(2)),
            ))?
            .label(response.spec.label)
            .legend(move |(x, y)| {
                PathElement::new(
                    [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                    response.spec.color.stroke_width(scaled(2)),
                )
            });
    }
    chart
        .configure_series_labels()
        .label_font(("sans-serif", scaled(12)))
        .margin(scaled(5))
        .legend_area_size(scaled(30))
        .background_style(WHITE)
        .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
        .position(SeriesLabelPosition::LowerLeft)
        .draw()?;
    Ok(())
}

fn draw_phase(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
    view: View,
) -> Result<(), Box<dyn Error>> {
    let start = responses[0].frequency[0].ratio;
    let end = responses[0].frequency.last().unwrap().ratio;
    let (minimum, maximum) = responses
        .iter()
        .flat_map(|response| {
            response
                .frequency
                .iter()
                .map(|point| point.phase_radians.to_degrees())
        })
        .fold((0.0_f64, 0.0_f64), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    let padding = ((maximum - minimum) * 0.03).max(5.0);
    let phase_floor = minimum - padding;
    let phase_ceiling = maximum + padding;
    let mut chart = ChartBuilder::on(area)
        .caption("Unwrapped phase response", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d((start..end).log_scale(), phase_floor..phase_ceiling)?;
    chart
        .configure_mesh()
        .x_desc(view.frequency_label())
        .y_desc("phase (degrees)")
        .x_labels(9)
        .y_labels(10)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(1.0, phase_floor), (1.0, phase_ceiling)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response
                .frequency
                .iter()
                .map(|point| (point.ratio, point.phase_radians.to_degrees())),
            response.spec.color.stroke_width(scaled(2)),
        ))?;
    }
    Ok(())
}

fn draw_group_delay(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
    view: View,
) -> Result<(), Box<dyn Error>> {
    let start = responses[0].frequency[0].ratio;
    let end = responses[0].frequency.last().unwrap().ratio;
    let minimum = responses
        .iter()
        .flat_map(|response| response.group_delay.iter().map(|(_, delay)| *delay))
        .filter(|delay| delay.is_finite())
        .fold(0.0_f64, f64::min);
    let maximum = responses
        .iter()
        .flat_map(|response| response.group_delay.iter().map(|(_, delay)| *delay))
        .filter(|delay| delay.is_finite() && *delay >= 0.0)
        .fold(1.0_f64, f64::max);
    let upper = maximum.mul_add(1.05, 0.05);
    let mut chart = ChartBuilder::on(area)
        .caption("Group delay", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d((start..end).log_scale(), minimum.min(-0.05)..upper)?;
    chart
        .configure_mesh()
        .x_desc(view.frequency_label())
        .y_desc(if view == View::BandPass {
            "normalized delay (ωcenter τg)"
        } else {
            "normalized delay (ωc τg)"
        })
        .x_labels(9)
        .y_labels(10)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(1.0, 0.0), (1.0, upper)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response.group_delay.iter().copied(),
            response.spec.color.stroke_width(scaled(2)),
        ))?;
    }
    Ok(())
}

fn draw_step(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
    view: View,
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
    let minimum = responses
        .iter()
        .flat_map(|response| response.step.iter().map(|(_, value)| *value))
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::min);
    let padding = ((maximum - minimum) * 0.05).max(0.05);
    let mut chart = ChartBuilder::on(area)
        .caption("Unit-step response", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(0.0_f64..end, minimum - padding..maximum + padding)?;
    chart
        .configure_mesh()
        .x_desc(if view == View::BandPass {
            "time × band center frequency"
        } else {
            "time × cutoff frequency"
        })
        .y_desc("output")
        .x_labels(9)
        .y_labels(10)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(0.0, view.step_reference()), (end, view.step_reference())],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    for response in responses {
        chart.draw_series(LineSeries::new(
            response.step.iter().copied(),
            response.spec.color.stroke_width(scaled(2)),
        ))?;
    }
    Ok(())
}

fn frequency_ratios() -> impl Iterator<Item = f64> {
    frequency_ratios_until(MAX_FREQUENCY_RATIO)
}

fn frequency_ratios_until(maximum: f64) -> impl Iterator<Item = f64> {
    let start = MIN_FREQUENCY_RATIO.log10();
    let end = maximum.log10();
    (0..FREQUENCY_POINTS).map(move |index| {
        let fraction = f64::from(index) / f64::from(FREQUENCY_POINTS - 1);
        10.0_f64.powf(start + fraction * (end - start))
    })
}

fn visible_magnitude_points(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let mut visible = Vec::with_capacity(points.len());
    for pair in points.windows(2) {
        let preceding = &pair[0];
        let current = &pair[1];
        if preceding.gain_db >= MAGNITUDE_FLOOR_DB && visible.is_empty() {
            visible.push((preceding.ratio, preceding.gain_db));
        }
        if (preceding.gain_db >= MAGNITUDE_FLOOR_DB) != (current.gain_db >= MAGNITUDE_FLOOR_DB) {
            let fraction =
                (MAGNITUDE_FLOOR_DB - preceding.gain_db) / (current.gain_db - preceding.gain_db);
            let log_ratio = fraction.mul_add(
                current.ratio.ln() - preceding.ratio.ln(),
                preceding.ratio.ln(),
            );
            visible.push((log_ratio.exp(), MAGNITUDE_FLOOR_DB));
        }
        if current.gain_db >= MAGNITUDE_FLOOR_DB {
            visible.push((current.ratio, current.gain_db));
        }
    }
    visible
}

fn exact_frequency_point<const N: usize>(
    response: Response,
    frequency_ratio: f64,
) -> Result<FrequencyPoint, Box<dyn Error>> {
    exact_frequency_point_for_order(response, N, frequency_ratio)
}

fn topology_frequency_point<const N: usize>(
    view: View,
    response: Response,
    frequency_ratio: f64,
) -> Result<FrequencyPoint, Box<dyn Error>> {
    match view {
        View::LowPass | View::Phase | View::Waveform => {
            exact_frequency_point::<N>(response, frequency_ratio)
        }
        View::HighPass => {
            let mut point = exact_frequency_point::<N>(response, 1.0 / frequency_ratio)?;
            point.ratio = frequency_ratio;
            point.phase_radians = -point.phase_radians;
            Ok(point)
        }
        View::BandPass => {
            // Low-pass to band-pass substitution with edges at 0.5 and 2.0.
            let mapped = (frequency_ratio - 1.0 / frequency_ratio) / 1.5;
            let mut point = exact_frequency_point_for_order(response, N / 2, mapped.abs())?;
            point.ratio = frequency_ratio;
            point.phase_radians *= mapped.signum();
            Ok(point)
        }
    }
}

fn exact_frequency_point_for_order(
    response: Response,
    order_usize: usize,
    frequency_ratio: f64,
) -> Result<FrequencyPoint, Box<dyn Error>> {
    let order = order_usize.to_f64().ok_or("order does not fit in f64")?;
    let mut log_magnitude = 0.0;
    let mut phase_radians = 0.0;
    match response {
        Response::RepeatedPole => {
            let rate = 1.0 / (core::f64::consts::LN_2 / order).exp_m1().sqrt();
            for _ in 0..order_usize {
                accumulate_first_order(
                    rate,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Butterworth => {
            if order_usize % 2 == 1 {
                accumulate_first_order(
                    1.0,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for index in 0..order_usize / 2 {
                let angle =
                    core::f64::consts::PI * (2 * index + 1).to_f64().unwrap() / (2.0 * order);
                accumulate_second_order(
                    2.0 * angle.sin(),
                    1.0,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Bessel => {
            let prototype = bessel_table::prototype(order_usize)
                .ok_or("Bessel response order exceeds the supported table")?;
            if order_usize % 2 == 1 {
                accumulate_first_order(
                    prototype.real_rate,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for (&damping, &frequency_squared) in
                prototype.damping.iter().zip(prototype.frequency_squared)
            {
                accumulate_second_order(
                    damping,
                    frequency_squared,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Chebyshev1 { ripple_db } => {
            let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
            let epsilon = epsilon_squared.sqrt();
            let mu = (1.0 / epsilon).asinh() / order;
            let cutoff_target = if order_usize % 2 == 0 {
                (1.0 / epsilon_squared + 2.0).sqrt()
            } else {
                1.0 / epsilon
            };
            let cutoff_scale = (cutoff_target.acosh() / order).cosh();
            if order_usize % 2 == 1 {
                accumulate_first_order(
                    mu.sinh() / cutoff_scale,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for index in 0..order_usize / 2 {
                let angle =
                    core::f64::consts::PI * (2 * index + 1).to_f64().unwrap() / (2.0 * order);
                let real = mu.sinh() * angle.sin() / cutoff_scale;
                let imaginary = mu.cosh() * angle.cos() / cutoff_scale;
                accumulate_second_order(
                    2.0 * real,
                    real.mul_add(real, imaginary * imaginary),
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        _ => return Err("response is not supported by this development plot".into()),
    }

    Ok(FrequencyPoint {
        ratio: frequency_ratio,
        gain_db: 20.0 / core::f64::consts::LN_10 * log_magnitude,
        phase_radians,
    })
}

fn accumulate_first_order(rate: f64, frequency: f64, log_magnitude: &mut f64, phase: &mut f64) {
    *log_magnitude += rate.ln() - rate.hypot(frequency).ln();
    *phase -= frequency.atan2(rate);
}

fn accumulate_second_order(
    damping: f64,
    natural_frequency_squared: f64,
    frequency: f64,
    log_magnitude: &mut f64,
    phase: &mut f64,
) {
    let denominator_real = natural_frequency_squared - frequency * frequency;
    let denominator_imaginary = damping * frequency;
    *log_magnitude +=
        natural_frequency_squared.ln() - denominator_real.hypot(denominator_imaginary).ln();
    *phase -= denominator_imaginary.atan2(denominator_real);
}

fn group_delay(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let width = 2 * GROUP_DELAY_HALF_WINDOW + 1;
    points
        .windows(width)
        .filter_map(|window| {
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
            delay.is_finite().then_some((center.ratio, delay))
        })
        .collect()
}

fn measure_step_response<const N: usize>(
    view: View,
    response: Response,
) -> Result<Vec<(f64, f64)>, Box<dyn Error>> {
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let duration = 4.0_f64.max(order);
    let dt = duration / (CUTOFF_HZ * f64::from(TRANSIENT_POINTS - 1));
    let mut filter: Box<dyn ssfilt::StreamingFilter<Scalar = f64>> = match view {
        View::LowPass | View::Phase | View::Waveform => Box::new(
            LowPass::<N>::builder(CUTOFF_HZ)
                .response(response)
                .input_model(InputModel::CurrentHold)
                .build()?,
        ),
        View::HighPass => Box::new(
            HighPass::<N>::builder(CUTOFF_HZ)
                .response(response)
                .input_model(InputModel::CurrentHold)
                .build()?,
        ),
        View::BandPass => Box::new(
            BandPass::<N>::builder(0.5 * CUTOFF_HZ, 2.0 * CUTOFF_HZ)
                .response(response)
                .input_model(InputModel::CurrentHold)
                .build()?,
        ),
    };
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
        let resized_line;
        let line = if line.starts_with("<svg ") {
            let rendered_size = format!(
                "width=\"{}\" height=\"{}\"",
                scaled(DISPLAY_WIDTH),
                scaled(DISPLAY_HEIGHT)
            );
            let display_size = format!("width=\"{DISPLAY_WIDTH}\" height=\"{DISPLAY_HEIGHT}\"");
            resized_line = line.replacen(&rendered_size, &display_size, 1);
            resized_line.as_str()
        } else {
            line
        };
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
    use approx::assert_relative_eq;

    use super::{
        MAGNITUDE_FLOOR_DB, PhaseModel, View, collapse_vertical_pixels, exact_frequency_point,
        frequency_ratios, pchip_path, smooth_polyline, topology_frequency_point,
        visible_magnitude_points,
    };
    use ssfilt::Response;

    #[test]
    fn phase_view_models_map_to_all_supported_response_families() {
        for (name, expected) in [
            ("repeated", PhaseModel::RepeatedPole),
            ("butterworth", PhaseModel::Butterworth),
            ("bessel", PhaseModel::Bessel),
            ("chebyshev", PhaseModel::Chebyshev1),
        ] {
            assert_eq!(PhaseModel::parse(name).unwrap(), expected);
        }
        assert!(PhaseModel::parse("unknown").is_err());
    }

    #[test]
    fn frequency_grid_is_logarithmically_spaced() {
        let ratios = frequency_ratios().collect::<Vec<_>>();
        let expected_ratio = ratios[1] / ratios[0];
        for pair in ratios.windows(2) {
            assert_relative_eq!(pair[1] / pair[0], expected_ratio, epsilon = 2.0e-15);
        }
    }

    #[test]
    fn exact_responses_share_the_minus_three_db_cutoff() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let point = exact_frequency_point::<36>(response, 1.0).unwrap();
            assert_relative_eq!(point.gain_db, -3.010_299_956_639_812, epsilon = 2.0e-11);
            assert!(point.phase_radians.is_finite());
        }

        let bessel = exact_frequency_point::<25>(Response::Bessel, 1.0).unwrap();
        assert_relative_eq!(bessel.gain_db, -3.010_299_956_639_812, epsilon = 2.0e-11);
        assert!(bessel.phase_radians.is_finite());
    }

    #[test]
    fn exact_high_order_stopbands_reach_the_plot_floor() {
        for response in [
            Response::Butterworth,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let point = exact_frequency_point::<36>(response, 20.0).unwrap();
            assert!(point.gain_db < MAGNITUDE_FLOOR_DB);
        }
    }

    #[test]
    fn magnitude_curve_ends_at_the_visible_floor() {
        let points = frequency_ratios()
            .map(|ratio| {
                exact_frequency_point::<36>(Response::Chebyshev1 { ripple_db: 0.5 }, ratio)
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let visible = visible_magnitude_points(&points);
        assert_eq!(visible.last().unwrap().1, MAGNITUDE_FLOOR_DB);
        assert!(visible.len() < points.len());
    }

    #[test]
    fn high_pass_and_band_pass_edges_reach_minus_three_db() {
        let target = -3.010_299_956_639_812;
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let high = topology_frequency_point::<4>(View::HighPass, response, 1.0).unwrap();
            assert_relative_eq!(high.gain_db, target, epsilon = 1.0e-10);
            assert!(high.phase_radians > 0.0);
            for edge in [0.5, 2.0] {
                let band = topology_frequency_point::<4>(View::BandPass, response, edge).unwrap();
                assert_relative_eq!(band.gain_db, target, epsilon = 1.0e-10);
            }
            let center = topology_frequency_point::<4>(View::BandPass, response, 1.0).unwrap();
            assert_relative_eq!(center.gain_db, 0.0, epsilon = 1.0e-12);
            assert_relative_eq!(center.phase_radians, 0.0, epsilon = 1.0e-12);
        }
    }

    #[test]
    fn band_pass_curve_enters_and_leaves_visible_range() {
        let points = frequency_ratios()
            .map(|ratio| {
                topology_frequency_point::<36>(View::BandPass, Response::Butterworth, ratio)
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let visible = visible_magnitude_points(&points);
        assert_eq!(visible.first().unwrap().1, MAGNITUDE_FLOOR_DB);
        assert_eq!(visible.last().unwrap().1, MAGNITUDE_FLOOR_DB);
        assert!(visible.len() < points.len());
    }

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
