use std::error::Error;
use std::path::Path;

use anyhow::Result;
use plotters::coord::Shift;
use plotters::coord::types::RangedCoordf64;
use plotters::prelude::*;

use crate::Topology;
use crate::analytic::{self, FrequencyPoint, MAGNITUDE_FLOOR_DB, visible_magnitude_points};
use crate::measure::{DEFAULT_SAMPLES_PER_CYCLE, InspectionPoint};

pub(super) const RESPONSE_STROKES: [&str; 4] = ["#2E6FD6", "#E05B4A", "#8752A1", "#249D5C"];
pub(super) const DISPLAY_WIDTH: u32 = 1_600;
pub(super) const DISPLAY_HEIGHT: u32 = 1_180;
const RENDER_SCALE: u32 = 4;
const MAGNITUDE_CEILING_DB: f64 = 5.0;

pub(super) const fn scaled(value: u32) -> u32 {
    value * RENDER_SCALE
}

#[derive(Clone, Copy)]
pub(super) struct ResponseSpec {
    pub(super) label: &'static str,
    pub(super) response: ssfilt::Response,
    pub(super) color: RGBColor,
}

impl ResponseSpec {
    pub(super) const fn new(
        label: &'static str,
        response: ssfilt::Response,
        color: RGBColor,
    ) -> Self {
        Self {
            label,
            response,
            color,
        }
    }
}

pub(super) struct ResponseData {
    pub(super) spec: ResponseSpec,
    pub(super) frequency: Vec<FrequencyPoint>,
    pub(super) measurements: Vec<InspectionPoint>,
    pub(super) step: Vec<(f64, f64)>,
}

pub(super) fn draw<const N: usize>(
    output_path: &Path,
    topology: Topology,
    responses: &[ResponseData],
) -> Result<()> {
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let order_label = match topology {
        Topology::BandPass => format!(
            "final order {N} (prototype order {})",
            topology.prototype_order(N)
        ),
        Topology::LowPass | Topology::HighPass => format!("order {N}"),
    };
    let root = root.titled(
        &format!(
            "ssfilt {} {order_label} — streamed markers use Linear reconstruction at {DEFAULT_SAMPLES_PER_CYCLE} samples/cycle",
            topology.label()
        ),
        ("sans-serif", scaled(30)),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_magnitude(&panels[0], topology, responses)?;
    draw_phase(&panels[1], topology, responses)?;
    draw_group_delay(&panels[2], topology, responses)?;
    draw_step(&panels[3], topology, responses)?;
    root.present()?;
    drop(panels);
    drop(root);
    Ok(())
}

fn draw_magnitude(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    topology: Topology,
    responses: &[ResponseData],
) -> Result<()> {
    let (minimum, maximum) = analytic::frequency_range(topology);
    let mut chart = ChartBuilder::on(area)
        .caption(magnitude_caption(topology), ("sans-serif", scaled(18)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(
            (minimum..maximum).log_scale(),
            MAGNITUDE_FLOOR_DB..MAGNITUDE_CEILING_DB,
        )?;
    configure_frequency_mesh(&mut chart, topology, gain_label(topology))?;
    chart.draw_series(LineSeries::new(
        [
            (minimum, -3.010_299_956_639_812),
            (maximum, -3.010_299_956_639_812),
        ],
        BLACK.mix(0.25).stroke_width(RENDER_SCALE),
    ))?;
    for edge in cutoff_edges(topology) {
        chart.draw_series(LineSeries::new(
            [(*edge, MAGNITUDE_FLOOR_DB), (*edge, MAGNITUDE_CEILING_DB)],
            BLACK.mix(0.25).stroke_width(RENDER_SCALE),
        ))?;
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
                    [
                        (x, y),
                        (
                            x + i32::try_from(scaled(24)).expect("small legend size fits"),
                            y,
                        ),
                    ],
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

fn draw_magnitude_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
) -> Result<()> {
    chart.draw_series(response.measurements.iter().filter_map(|inspection| {
        inspection
            .measured
            .filter(|point| {
                point.gain_db >= MAGNITUDE_FLOOR_DB && point.gain_db <= MAGNITUDE_CEILING_DB
            })
            .map(|point| {
                Circle::new(
                    (inspection.ratio, point.gain_db),
                    scaled(3),
                    response.spec.color.filled(),
                )
            })
    }))?;
    Ok(())
}

fn draw_phase(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    topology: Topology,
    responses: &[ResponseData],
) -> Result<()> {
    let (minimum, maximum) = analytic::frequency_range(topology);
    let (phase_minimum, phase_maximum) = phase_range(responses);
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Unwrapped phase: analytic curves and public-stream inspection markers where within work limit",
            ("sans-serif", scaled(18)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d((minimum..maximum).log_scale(), phase_minimum..phase_maximum)?;
    configure_frequency_mesh(&mut chart, topology, "phase (degrees)")?;
    for edge in cutoff_edges(topology) {
        chart.draw_series(LineSeries::new(
            [(*edge, phase_minimum), (*edge, phase_maximum)],
            BLACK.mix(0.25).stroke_width(RENDER_SCALE),
        ))?;
    }
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

fn draw_phase_markers(
    chart: &mut ChartContext<'_, SVGBackend<'_>, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    response: &ResponseData,
) -> Result<()> {
    chart.draw_series(response.measurements.iter().filter_map(|inspection| {
        inspection
            .measured
            .filter(|point| point.phase_is_resolved())
            .map(|point| {
                Circle::new(
                    (inspection.ratio, point.unwrapped_phase_radians.to_degrees()),
                    scaled(3),
                    response.spec.color.filled(),
                )
            })
    }))?;
    Ok(())
}

fn draw_group_delay(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    topology: Topology,
    responses: &[ResponseData],
) -> Result<()> {
    let (minimum, maximum) = analytic::frequency_range(topology);
    let delay_maximum = responses
        .iter()
        .flat_map(|response| {
            response
                .frequency
                .iter()
                .map(|point| point.normalized_group_delay)
        })
        .fold(0.5_f64, f64::max);
    let upper = delay_maximum.mul_add(1.05, 0.05);
    let mut chart = ChartBuilder::on(area)
        .caption("Normalized group delay", ("sans-serif", scaled(18)))
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(68))
        .build_cartesian_2d((minimum..maximum).log_scale(), 0.0_f64..upper)?;
    configure_frequency_mesh(&mut chart, topology, "normalized group delay (ω₀ τg)")?;
    for edge in cutoff_edges(topology) {
        chart.draw_series(LineSeries::new(
            [(*edge, 0.0), (*edge, upper)],
            BLACK.mix(0.25).stroke_width(RENDER_SCALE),
        ))?;
    }
    for response in responses {
        chart.draw_series(LineSeries::new(
            response
                .frequency
                .iter()
                .map(|point| (point.ratio, point.normalized_group_delay)),
            response.spec.color.stroke_width(scaled(2)),
        ))?;
    }
    Ok(())
}

fn draw_step(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    topology: Topology,
    responses: &[ResponseData],
) -> Result<()> {
    let end = responses
        .first()
        .and_then(|response| response.step.last())
        .map_or(4.0, |(time, _)| *time);
    let steady_value = topology.steady_step_value();
    let (minimum, maximum) = responses
        .iter()
        .flat_map(|response| response.step.iter().map(|(_, value)| *value))
        .fold((steady_value, steady_value), |(low, high), value| {
            (low.min(value), high.max(value))
        });
    let span = (maximum - minimum).max(0.25);
    let lower = minimum - 0.08 * span;
    let upper = maximum + 0.08 * span;
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Unit-step response from streamed updates",
            ("sans-serif", scaled(18)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(46))
        .y_label_area_size(scaled(62))
        .build_cartesian_2d(0.0_f64..end, lower..upper)?;
    chart
        .configure_mesh()
        .x_desc(match topology {
            Topology::LowPass | Topology::HighPass => "time × whole-filter cutoff frequency",
            Topology::BandPass => "time × geometric band-center frequency",
        })
        .y_desc("output")
        .x_labels(9)
        .y_labels(8)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    chart.draw_series(LineSeries::new(
        [(0.0, steady_value), (end, steady_value)],
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

fn configure_frequency_mesh<DB: DrawingBackend>(
    chart: &mut ChartContext<'_, DB, Cartesian2d<LogCoord<f64>, RangedCoordf64>>,
    topology: Topology,
    y_label: &str,
) -> Result<()>
where
    DB::ErrorType: Error + 'static,
{
    let x_label = match topology {
        Topology::BandPass => "frequency / geometric band center",
        Topology::LowPass | Topology::HighPass => "frequency / whole-filter −3 dB cutoff",
    };
    chart
        .configure_mesh()
        .x_desc(x_label)
        .y_desc(y_label)
        .x_labels(9)
        .y_labels(9)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(14)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    Ok(())
}

fn gain_label(topology: Topology) -> &'static str {
    match topology {
        Topology::LowPass => "gain relative to DC (dB)",
        Topology::HighPass | Topology::BandPass => "gain relative to passband (dB)",
    }
}

fn magnitude_caption(topology: Topology) -> &'static str {
    match topology {
        Topology::BandPass => {
            "Magnitude: analytic curves and public-stream inspection markers where within work limit (whole-filter −3 dB edges at 0.5 and 2 Hz)"
        }
        Topology::LowPass | Topology::HighPass => {
            "Magnitude: analytic curves and public-stream inspection markers where within work limit (whole-filter −3 dB cutoff at 1 Hz)"
        }
    }
}

fn cutoff_edges(topology: Topology) -> &'static [f64] {
    match topology {
        Topology::LowPass | Topology::HighPass => &[1.0],
        Topology::BandPass => &[0.5, 2.0],
    }
}

fn phase_range(responses: &[ResponseData]) -> (f64, f64) {
    let (minimum, maximum) = responses
        .iter()
        .flat_map(|response| {
            response
                .frequency
                .iter()
                .map(|point| point.phase_radians.to_degrees())
                .chain(
                    response
                        .measurements
                        .iter()
                        .filter_map(|point| point.measured)
                        .filter(|point| point.phase_is_resolved())
                        .map(|point| point.unwrapped_phase_radians.to_degrees()),
                )
        })
        .fold((0.0_f64, 0.0_f64), |(low, high), value| {
            (low.min(value), high.max(value))
        });
    let span = (maximum - minimum).max(20.0);
    (minimum - 0.08 * span, maximum + 0.08 * span)
}
