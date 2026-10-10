use std::error::Error;
use std::path::Path;

use num_traits::ToPrimitive;
use plotters::coord::Shift;
use plotters::coord::types::RangedCoordf64;
use plotters::prelude::*;

use super::analytic::{
    FrequencyPoint, MAGNITUDE_FLOOR_DB, MAX_FREQUENCY_RATIO, MIN_FREQUENCY_RATIO,
    visible_magnitude_points,
};
use super::measure::{MeasuredFrequencyPoint, SampleSchedule};

pub(super) const RESPONSE_STROKES: [&str; 4] = ["#2E6FD6", "#E05B4A", "#249D5C", "#8752A1"];
pub(super) const DISPLAY_WIDTH: u32 = 1_600;
pub(super) const DISPLAY_HEIGHT: u32 = 1_180;
const RENDER_SCALE: u32 = 8;

pub(super) const fn scaled(value: u32) -> u32 {
    value * RENDER_SCALE
}

#[derive(Clone, Copy)]
pub(super) struct ResponseSpec {
    pub(super) label: &'static str,
    pub(super) response: ssfilt::Response,
    pub(super) color: RGBColor,
}

pub(super) struct ResponseData {
    pub(super) spec: ResponseSpec,
    pub(super) frequency: Vec<FrequencyPoint>,
    pub(super) measurements: Vec<MeasuredFrequencyPoint>,
    pub(super) group_delay: Vec<(f64, f64)>,
    pub(super) step: Vec<(f64, f64)>,
}

pub(super) fn draw<const N: usize>(
    output_path: &Path,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!("ssfilt order {N} low-pass responses"),
        ("sans-serif", scaled(30)),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_magnitude(&panels[0], responses)?;
    draw_phase::<N>(&panels[1], responses)?;
    draw_group_delay(&panels[2], responses)?;
    draw_step(&panels[3], responses)?;
    root.present()?;
    drop(panels);
    drop(root);
    Ok(())
}

fn draw_magnitude(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Magnitude response — lines: analytic; dots: uniform; rings: jittered",
            ("sans-serif", scaled(18)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
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
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(MIN_FREQUENCY_RATIO, -3.0), (MAX_FREQUENCY_RATIO, -3.0)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    chart.draw_series(LineSeries::new(
        [(1.0, MAGNITUDE_FLOOR_DB), (1.0, 5.0)],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
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
        draw_magnitude_markers(&mut chart, response)?;
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

fn draw_phase<const N: usize>(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error>> {
    let phase_floor = -90.0 * N.to_f64().ok_or("order does not fit in f64")?;
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Phase on analytic branch — lines: analytic; dots: uniform; rings: jittered",
            ("sans-serif", scaled(18)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
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
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(1.0, phase_floor), (1.0, 5.0)],
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
        draw_phase_markers(&mut chart, response)?;
    }
    Ok(())
}

fn draw_magnitude_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
) -> Result<(), Box<dyn Error>> {
    chart.draw_series(
        response
            .measurements
            .iter()
            .filter(|point| point.streamed.schedule == SampleSchedule::Uniform)
            .filter_map(|point| {
                let gain_db = point.measured_gain_db();
                (gain_db >= MAGNITUDE_FLOOR_DB).then_some(Circle::new(
                    (point.streamed.frequency_ratio, gain_db),
                    scaled(2),
                    response.spec.color.filled(),
                ))
            }),
    )?;
    chart.draw_series(
        response
            .measurements
            .iter()
            .filter(|point| point.streamed.schedule == SampleSchedule::BoundedJitter)
            .filter_map(|point| {
                let gain_db = point.measured_gain_db();
                (gain_db >= MAGNITUDE_FLOOR_DB).then_some(Circle::new(
                    (point.streamed.frequency_ratio, gain_db),
                    scaled(4),
                    response.spec.color.stroke_width(scaled(1)),
                ))
            }),
    )?;
    Ok(())
}

fn draw_phase_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
) -> Result<(), Box<dyn Error>> {
    chart.draw_series(
        response
            .measurements
            .iter()
            .filter(|point| {
                point.streamed.schedule == SampleSchedule::Uniform
                    && point.streamed.phase_is_resolved()
            })
            .map(|point| {
                Circle::new(
                    (
                        point.streamed.frequency_ratio,
                        point.unwrapped_phase_radians.to_degrees(),
                    ),
                    scaled(2),
                    response.spec.color.filled(),
                )
            }),
    )?;
    chart.draw_series(
        response
            .measurements
            .iter()
            .filter(|point| {
                point.streamed.schedule == SampleSchedule::BoundedJitter
                    && point.streamed.phase_is_resolved()
            })
            .map(|point| {
                Circle::new(
                    (
                        point.streamed.frequency_ratio,
                        point.unwrapped_phase_radians.to_degrees(),
                    ),
                    scaled(4),
                    response.spec.color.stroke_width(scaled(1)),
                )
            }),
    )?;
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
        .caption("Group delay", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
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
        .caption("Unit-step response", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(0.0_f64..end, -0.1_f64..maximum.mul_add(1.05, 0.05))?;
    chart
        .configure_mesh()
        .x_desc("time × cutoff frequency")
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
        [(0.0, 1.0), (end, 1.0)],
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
