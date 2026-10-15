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
    pub(super) phase_equalized: Option<PhaseEqualizedData>,
}

pub(super) struct PhaseEqualizedData {
    pub(super) frequency: Vec<FrequencyPoint>,
    pub(super) measurements: Vec<MeasuredFrequencyPoint>,
    pub(super) step: Vec<(f64, f64)>,
    pub(super) validated_band_edge_ratio: f64,
    pub(super) target_normalized_delay: f64,
    pub(super) mean_added_normalized_delay: f64,
}

pub(super) fn draw<const N: usize>(
    output_path: &Path,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!("ssfilt order {N} low-pass response evidence"),
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let caption = if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        "Magnitude — lines analytic; circles/triangles source/equalized; fill/open uniform/jitter"
    } else {
        "Magnitude — analytic lines; streamed fits (filled uniform, open jitter)"
    };
    let mut chart = ChartBuilder::on(area)
        .caption(caption, ("sans-serif", scaled(15)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            MAGNITUDE_FLOOR_DB..5.0_f64,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency ratio (f/fc)")
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
        if let Some(equalized) = &response.phase_equalized {
            draw_equalized_magnitude_markers(&mut chart, response, equalized)?;
        }
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let source_asymptote = -90.0 * N.to_f64().ok_or("order does not fit in f64")?;
    let minimum_phase = responses
        .iter()
        .flat_map(|response| {
            response.frequency.iter().chain(
                response
                    .phase_equalized
                    .iter()
                    .flat_map(|equalized| &equalized.frequency),
            )
        })
        .map(|point| point.phase_radians.to_degrees())
        .filter(|phase| phase.is_finite())
        .fold(source_asymptote, f64::min);
    let phase_floor = minimum_phase * 1.02;
    let caption = if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        "Phase — lines analytic; circles/triangles source/equalized; claim ends at 0.7 fc"
    } else {
        "Phase on analytic branch — analytic lines; streamed fits (filled uniform, open jitter)"
    };
    let mut chart = ChartBuilder::on(area)
        .caption(caption, ("sans-serif", scaled(15)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            phase_floor..5.0_f64,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency ratio (f/fc)")
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
        let source = chart.draw_series(LineSeries::new(
            response
                .frequency
                .iter()
                .map(|point| (point.ratio, point.phase_radians.to_degrees())),
            response.spec.color.mix(0.55).stroke_width(scaled(2)),
        ))?;
        if response.phase_equalized.is_some() {
            source
                .label(format!("{} source", response.spec.label))
                .legend(move |(x, y)| {
                    PathElement::new(
                        [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                        response.spec.color.mix(0.55).stroke_width(scaled(2)),
                    )
                });
        }
        draw_phase_markers(&mut chart, response)?;
        if let Some(equalized) = &response.phase_equalized {
            draw_equalized_phase(&mut chart, response, equalized, phase_floor)?;
        }
    }
    if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        chart
            .configure_series_labels()
            .label_font(("sans-serif", scaled(12)))
            .margin(scaled(5))
            .legend_area_size(scaled(30))
            .background_style(WHITE)
            .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
            .position(SeriesLabelPosition::UpperRight)
            .draw()?;
    }
    Ok(())
}

fn draw_equalized_magnitude_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
    equalized: &PhaseEqualizedData,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    chart.draw_series(equalized.measurements.iter().filter_map(|point| {
        let gain_db = point.measured_gain_db();
        (gain_db >= MAGNITUDE_FLOOR_DB).then_some(TriangleMarker::new(
            (point.streamed.frequency_ratio, gain_db),
            if point.streamed.schedule == SampleSchedule::Uniform {
                scaled(3)
            } else {
                scaled(5)
            },
            if point.streamed.schedule == SampleSchedule::Uniform {
                response.spec.color.filled()
            } else {
                response.spec.color.stroke_width(scaled(1))
            },
        ))
    }))?;
    Ok(())
}

fn draw_equalized_phase(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
    equalized: &PhaseEqualizedData,
    phase_floor: f64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let edge = equalized.validated_band_edge_ratio;
    let color = response.spec.color;
    let label = format!("{} + all-pass", response.spec.label);
    chart.draw_series(LineSeries::new(
        [(edge, phase_floor), (edge, 5.0)],
        color.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    let validated = equalized
        .frequency
        .iter()
        .take_while(|point| point.ratio <= edge)
        .map(|point| (point.ratio, point.phase_radians.to_degrees()))
        .collect::<Vec<_>>();
    chart
        .draw_series(LineSeries::new(validated, color.stroke_width(scaled(3))))?
        .label(label)
        .legend(move |(x, y)| {
            PathElement::new(
                [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                color.stroke_width(scaled(3)),
            )
        });
    let diagnostic = equalized
        .frequency
        .iter()
        .filter(|point| point.ratio >= edge)
        .map(|point| (point.ratio, point.phase_radians.to_degrees()))
        .collect::<Vec<_>>();
    chart.draw_series(LineSeries::new(
        diagnostic,
        color.mix(0.25).stroke_width(scaled(2)),
    ))?;
    let markers = equalized
        .measurements
        .iter()
        .filter(|point| point.phase_is_resolved())
        .map(|point| {
            let opacity = if point.streamed.frequency_ratio <= edge {
                1.0
            } else {
                0.3
            };
            TriangleMarker::new(
                (
                    point.streamed.frequency_ratio,
                    point.unwrapped_phase_radians.to_degrees(),
                ),
                if point.streamed.schedule == SampleSchedule::Uniform {
                    scaled(3)
                } else {
                    scaled(5)
                },
                if point.streamed.schedule == SampleSchedule::Uniform {
                    color.mix(opacity).filled()
                } else {
                    color.mix(opacity).stroke_width(scaled(1))
                },
            )
        })
        .collect::<Vec<_>>();
    chart.draw_series(markers)?;
    Ok(())
}

fn draw_magnitude_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
) -> Result<(), Box<dyn Error + Send + Sync>> {
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    chart.draw_series(
        response
            .measurements
            .iter()
            .filter(|point| {
                point.streamed.schedule == SampleSchedule::Uniform && point.phase_is_resolved()
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
                    && point.phase_is_resolved()
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let maximum = responses
        .iter()
        .flat_map(|response| {
            response.group_delay.iter().map(|(_, delay)| *delay).chain(
                response
                    .phase_equalized
                    .iter()
                    .flat_map(|equalized| &equalized.frequency)
                    .map(|point| point.normalized_group_delay),
            )
        })
        .filter(|delay| delay.is_finite() && *delay >= 0.0)
        .fold(1.0_f64, f64::max);
    let upper = maximum.mul_add(1.05, 0.05);
    let caption = if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        "Analytic group delay — equalized claim ends at 0.7 fc"
    } else {
        "Analytic group delay"
    };
    let mut chart = ChartBuilder::on(area)
        .caption(caption, ("sans-serif", scaled(18)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d(
            (MIN_FREQUENCY_RATIO..MAX_FREQUENCY_RATIO).log_scale(),
            0.0_f64..upper,
        )?;
    chart
        .configure_mesh()
        .x_desc("frequency ratio (f/fc)")
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
        let source = chart.draw_series(LineSeries::new(
            response.group_delay.iter().copied(),
            response.spec.color.mix(0.55).stroke_width(scaled(2)),
        ))?;
        if response.phase_equalized.is_some() {
            source
                .label(format!("{} source", response.spec.label))
                .legend(move |(x, y)| {
                    PathElement::new(
                        [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                        response.spec.color.mix(0.55).stroke_width(scaled(2)),
                    )
                });
        }
        if let Some(equalized) = &response.phase_equalized {
            draw_equalized_group_delay(&mut chart, response, equalized, upper)?;
        }
    }
    if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        chart
            .configure_series_labels()
            .label_font(("sans-serif", scaled(12)))
            .margin(scaled(5))
            .legend_area_size(scaled(30))
            .background_style(WHITE)
            .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
            .position(SeriesLabelPosition::UpperRight)
            .draw()?;
    }
    Ok(())
}

fn draw_equalized_group_delay(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
    equalized: &PhaseEqualizedData,
    upper: f64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let edge = equalized.validated_band_edge_ratio;
    let color = response.spec.color;
    let label = format!("{} + all-pass", response.spec.label);
    let validated = equalized
        .frequency
        .iter()
        .take_while(|point| point.ratio <= edge)
        .map(|point| (point.ratio, point.normalized_group_delay))
        .collect::<Vec<_>>();
    let diagnostic = equalized
        .frequency
        .iter()
        .filter(|point| point.ratio >= edge)
        .map(|point| (point.ratio, point.normalized_group_delay))
        .collect::<Vec<_>>();

    chart.draw_series(LineSeries::new(
        [(edge, 0.0), (edge, upper)],
        color.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    chart
        .draw_series(LineSeries::new(validated, color.stroke_width(scaled(3))))?
        .label(label)
        .legend(move |(x, y)| {
            PathElement::new(
                [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                color.stroke_width(scaled(3)),
            )
        });
    chart.draw_series(LineSeries::new(
        diagnostic,
        color.mix(0.25).stroke_width(scaled(2)),
    ))?;
    chart
        .draw_series(LineSeries::new(
            [
                (MIN_FREQUENCY_RATIO, equalized.target_normalized_delay),
                (edge, equalized.target_normalized_delay),
            ],
            BLACK.mix(0.55).stroke_width(RENDER_SCALE),
        ))?
        .label("target constant delay")
        .legend(|(x, y)| {
            PathElement::new(
                [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                BLACK.mix(0.55).stroke_width(RENDER_SCALE),
            )
        });
    Ok(())
}

fn draw_step(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let end = responses
        .first()
        .and_then(|response| response.step.last())
        .map_or(4.0, |(time, _)| *time);
    let outputs = || {
        responses.iter().flat_map(|response| {
            response.step.iter().map(|(_, value)| *value).chain(
                response
                    .phase_equalized
                    .iter()
                    .flat_map(|equalized| &equalized.step)
                    .map(|(_, value)| *value),
            )
        })
    };
    let maximum = outputs()
        .filter(|value| value.is_finite())
        .fold(1.0_f64, f64::max);
    let minimum = outputs()
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::min);
    let lower = minimum.mul_add(1.05, -0.05);
    let upper = maximum.mul_add(1.05, 0.05);
    let mut chart = ChartBuilder::on(area)
        .caption("Unit-step response", ("sans-serif", scaled(22)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(0.0_f64..end, lower..upper)?;
    chart
        .configure_mesh()
        .x_desc("normalized time (t × cutoff frequency)")
        .y_desc("output for unit step")
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
        let source = chart.draw_series(LineSeries::new(
            response.step.iter().copied(),
            response.spec.color.mix(0.55).stroke_width(scaled(2)),
        ))?;
        if let Some(equalized) = &response.phase_equalized {
            source
                .label(format!("{} source", response.spec.label))
                .legend(move |(x, y)| {
                    PathElement::new(
                        [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                        response.spec.color.mix(0.55).stroke_width(scaled(2)),
                    )
                });
            chart
                .draw_series(LineSeries::new(
                    equalized.step.iter().copied(),
                    response.spec.color.stroke_width(scaled(3)),
                ))?
                .label(format!("{} + all-pass", response.spec.label))
                .legend(move |(x, y)| {
                    PathElement::new(
                        [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
                        response.spec.color.stroke_width(scaled(3)),
                    )
                });
        }
    }
    if responses
        .iter()
        .any(|response| response.phase_equalized.is_some())
    {
        chart
            .configure_series_labels()
            .label_font(("sans-serif", scaled(12)))
            .margin(scaled(5))
            .legend_area_size(scaled(30))
            .background_style(WHITE)
            .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
            .position(SeriesLabelPosition::LowerRight)
            .draw()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use plotters::prelude::RGBColor;
    use ssfilt::Response;

    use super::{PhaseEqualizedData, ResponseData, ResponseSpec, draw};
    use crate::analytic::FrequencyPoint;

    #[test]
    fn equalized_dashboard_discloses_validation_edge_and_visual_roles() {
        let point = |ratio, phase, delay| FrequencyPoint {
            ratio,
            gain_db: -3.0,
            phase_radians: phase,
            normalized_group_delay: delay,
        };
        let response = ResponseData {
            spec: ResponseSpec {
                label: "Butterworth",
                response: Response::Butterworth,
                color: RGBColor(224, 91, 74),
            },
            frequency: vec![point(0.01, -0.1, 2.0), point(20.0, -5.0, 0.1)],
            measurements: Vec::new(),
            group_delay: vec![(0.01, 2.0), (20.0, 0.1)],
            step: vec![(0.0, 0.0), (4.0, 1.0)],
            phase_equalized: Some(PhaseEqualizedData {
                frequency: vec![
                    point(0.01, -0.05, 6.6),
                    point(0.7, -2.0, 6.55),
                    point(20.0, -9.0, 0.2),
                ],
                measurements: Vec::new(),
                step: vec![(0.0, 0.0), (4.0, 0.9)],
                validated_band_edge_ratio: 0.7,
                target_normalized_delay: 6.6,
                mean_added_normalized_delay: 3.75,
            }),
        };
        let path =
            std::env::temp_dir().join(format!("ssfilt-response-render-{}.svg", std::process::id()));

        draw::<4>(&path, &[response]).unwrap();
        let svg = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert!(svg.contains("circles/triangles source/equalized"));
        assert!(svg.contains("equalized claim ends at 0.7 fc"));
        assert!(svg.contains("Butterworth source"));
        assert!(svg.contains("Butterworth + all-pass"));
        assert!(svg.contains("target constant delay"));
        assert!(svg.contains("output for unit step"));
    }

    #[test]
    fn ordinary_dashboard_does_not_claim_an_equalized_overlay() {
        let response = ResponseData {
            spec: ResponseSpec {
                label: "Butterworth",
                response: Response::Butterworth,
                color: RGBColor(224, 91, 74),
            },
            frequency: vec![FrequencyPoint {
                ratio: 1.0,
                gain_db: -3.0,
                phase_radians: -1.0,
                normalized_group_delay: 1.0,
            }],
            measurements: Vec::new(),
            group_delay: vec![(1.0, 1.0)],
            step: vec![(0.0, 0.0), (4.0, 1.0)],
            phase_equalized: None,
        };
        let path = std::env::temp_dir().join(format!(
            "ssfilt-source-response-render-{}.svg",
            std::process::id()
        ));

        draw::<4>(&path, &[response]).unwrap();
        let svg = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert!(svg.contains("Analytic group delay"));
        assert!(!svg.contains("equalized claim"));
        assert!(!svg.contains("triangles source/equalized"));
    }
}
