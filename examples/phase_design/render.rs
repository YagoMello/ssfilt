use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use plotters::coord::Shift;
use plotters::prelude::*;

use crate::search::{CaseResult, SweepPoint};

pub(super) const DISPLAY_WIDTH: u32 = 1_600;
pub(super) const DISPLAY_HEIGHT: u32 = 1_180;
pub(super) const RESPONSE_STROKES: [&str; 3] = ["#2E6FD6", "#E05B4A", "#249D5C"];

const RENDER_SCALE: u32 = 8;
const SOURCE_COLOR: RGBColor = RGBColor(46, 111, 214);
const CORRECTED_COLOR: RGBColor = RGBColor(224, 91, 74);
const EQUALIZER_COLOR: RGBColor = RGBColor(36, 157, 92);
const LIMIT_COLOR: RGBColor = RGBColor(92, 99, 112);
const MAX_PEAK_RESIDUAL: f64 = 0.10;
const MAX_PHASE_RESIDUAL_RADIANS: f64 = 0.05;
const MARKER_TARGET_COUNT: usize = 33;

pub(super) const fn scaled(value: u32) -> u32 {
    value * RENDER_SCALE
}

pub(super) fn write_dashboards(
    output_dir: &Path,
    results: &[CaseResult],
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    fs::create_dir_all(output_dir)?;
    let mut paths = Vec::with_capacity(results.len());
    for result in results {
        let path = output_dir.join(case_file_name(result));
        draw(&path, result)?;
        crate::svg_smoothing::smooth_svg_curves(&path)?;
        paths.push(path);
    }
    Ok(paths)
}

fn case_file_name(result: &CaseResult) -> String {
    format!(
        "butterworth-order-{}-{}.svg",
        result.case.order,
        result.case.band.label()
    )
}

fn draw(output_path: &Path, result: &CaseResult) -> Result<(), Box<dyn Error>> {
    let status = if result.gates.passes() {
        "ANALYTIC GATES PASS; NOT STREAMING"
    } else {
        "ANALYTIC GATES REJECT; NOT STREAMING"
    };
    let root = SVGBackend::new(output_path, (scaled(DISPLAY_WIDTH), scaled(DISPLAY_HEIGHT)))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let root = root.titled(
        &format!(
            "{} order {} — {} band 0..{} × cutoff — {status}",
            result.case.response_label,
            result.case.order,
            result.case.band.label(),
            result.case.band.upper_frequency_ratio(),
        ),
        ("sans-serif", scaled(28)),
    )?;
    let panels = root.split_evenly((2, 2));
    draw_total_delay(&panels[0], result)?;
    draw_delay_residual(&panels[1], result)?;
    draw_phase_residual(&panels[2], result)?;
    draw_added_delay(&panels[3], result)?;
    root.present()?;
    drop(panels);
    drop(root);
    Ok(())
}

fn draw_total_delay(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    result: &CaseResult,
) -> Result<(), Box<dyn Error>> {
    let points = &result.holdout_points;
    let minimum = points
        .iter()
        .flat_map(|point| [point.source_delay, point.total_delay])
        .fold(f64::INFINITY, f64::min);
    let maximum = points
        .iter()
        .flat_map(|point| [point.source_delay, point.total_delay])
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (maximum - minimum).max(0.1);
    let lower = (minimum - 0.07 * span).max(0.0);
    let upper = maximum + 0.07 * span;
    let x_upper = result.case.band.upper_frequency_ratio();
    let caption = if x_upper > 0.7 {
        "Group delay — analytic holdout records; shaded: 0.7..1.0 near cutoff"
    } else {
        "Group delay — curves and rings use stored analytic holdout records"
    };
    let mut chart = ChartBuilder::on(area)
        .caption(caption, ("sans-serif", scaled(17)))
        .margin(scaled(16))
        .x_label_area_size(scaled(48))
        .y_label_area_size(scaled(72))
        .build_cartesian_2d(0.0_f64..x_upper, lower..upper)?;
    configure_mesh(
        &mut chart,
        "normalized frequency (ω/ωc)",
        "normalized delay (ωc τg)",
    )?;
    shade_near_cutoff_region(&mut chart, x_upper, lower, upper)?;
    chart
        .draw_series(LineSeries::new(
            points
                .iter()
                .map(|point| (point.frequency_ratio, point.source_delay)),
            SOURCE_COLOR.stroke_width(scaled(2)),
        ))?
        .label("source")
        .legend(|(x, y)| legend_line(x, y, SOURCE_COLOR));
    chart
        .draw_series(LineSeries::new(
            points
                .iter()
                .map(|point| (point.frequency_ratio, point.total_delay)),
            CORRECTED_COLOR.stroke_width(scaled(2)),
        ))?
        .label("source + all-pass")
        .legend(|(x, y)| legend_line(x, y, CORRECTED_COLOR));
    chart
        .draw_series(LineSeries::new(
            [
                (0.0, result.corrected_metrics.target_delay),
                (x_upper, result.corrected_metrics.target_delay),
            ],
            LIMIT_COLOR.mix(0.7).stroke_width(RENDER_SCALE),
        ))?
        .label("minimax target D")
        .legend(|(x, y)| legend_line(x, y, LIMIT_COLOR));
    draw_markers(&mut chart, points, |point| point.source_delay, SOURCE_COLOR)?;
    draw_markers(
        &mut chart,
        points,
        |point| point.total_delay,
        CORRECTED_COLOR,
    )?;
    draw_legend(&mut chart)?;
    Ok(())
}

fn draw_delay_residual(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    result: &CaseResult,
) -> Result<(), Box<dyn Error>> {
    let points = &result.holdout_points;
    let maximum = points
        .iter()
        .flat_map(|point| {
            [
                (point.source_delay - result.source_metrics.target_delay).abs(),
                (point.total_delay - result.corrected_metrics.target_delay).abs(),
            ]
        })
        .fold(MAX_PEAK_RESIDUAL, f64::max);
    let x_upper = result.case.band.upper_frequency_ratio();
    let y_upper = maximum.mul_add(1.08, 0.01);
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Absolute delay residual — lower is flatter",
            ("sans-serif", scaled(19)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(48))
        .y_label_area_size(scaled(72))
        .build_cartesian_2d(0.0_f64..x_upper, 0.0_f64..y_upper)?;
    configure_mesh(&mut chart, "normalized frequency (ω/ωc)", "|τg − target|")?;
    shade_near_cutoff_region(&mut chart, x_upper, 0.0, y_upper)?;
    chart
        .draw_series(LineSeries::new(
            points.iter().map(|point| {
                (
                    point.frequency_ratio,
                    (point.source_delay - result.source_metrics.target_delay).abs(),
                )
            }),
            SOURCE_COLOR.stroke_width(scaled(2)),
        ))?
        .label("source")
        .legend(|(x, y)| legend_line(x, y, SOURCE_COLOR));
    chart
        .draw_series(LineSeries::new(
            points.iter().map(|point| {
                (
                    point.frequency_ratio,
                    (point.total_delay - result.corrected_metrics.target_delay).abs(),
                )
            }),
            CORRECTED_COLOR.stroke_width(scaled(2)),
        ))?
        .label("source + all-pass")
        .legend(|(x, y)| legend_line(x, y, CORRECTED_COLOR));
    chart
        .draw_series(LineSeries::new(
            [(0.0, MAX_PEAK_RESIDUAL), (x_upper, MAX_PEAK_RESIDUAL)],
            LIMIT_COLOR.stroke_width(RENDER_SCALE),
        ))?
        .label("corrected absolute gate 0.10")
        .legend(|(x, y)| legend_line(x, y, LIMIT_COLOR));
    draw_markers(
        &mut chart,
        points,
        |point| (point.total_delay - result.corrected_metrics.target_delay).abs(),
        CORRECTED_COLOR,
    )?;
    draw_legend(&mut chart)?;
    Ok(())
}

fn draw_phase_residual(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    result: &CaseResult,
) -> Result<(), Box<dyn Error>> {
    let points = &result.holdout_points;
    let maximum = points
        .iter()
        .map(|point| point.phase_residual_radians.abs())
        .fold(MAX_PHASE_RESIDUAL_RADIANS, f64::max)
        .mul_add(1.12, 0.005);
    let x_upper = result.case.band.upper_frequency_ratio();
    let mut chart = ChartBuilder::on(area)
        .caption(
            "Residual phase after removing the same minimax delay D",
            ("sans-serif", scaled(17)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(48))
        .y_label_area_size(scaled(72))
        .build_cartesian_2d(0.0_f64..x_upper, -maximum..maximum)?;
    configure_mesh(
        &mut chart,
        "normalized frequency (ω/ωc)",
        "phase residual (rad)",
    )?;
    shade_near_cutoff_region(&mut chart, x_upper, -maximum, maximum)?;
    chart
        .draw_series(LineSeries::new(
            points
                .iter()
                .map(|point| (point.frequency_ratio, point.phase_residual_radians)),
            CORRECTED_COLOR.stroke_width(scaled(2)),
        ))?
        .label("source + all-pass residual")
        .legend(|(x, y)| legend_line(x, y, CORRECTED_COLOR));
    chart
        .draw_series(LineSeries::new(
            [
                (0.0, MAX_PHASE_RESIDUAL_RADIANS),
                (x_upper, MAX_PHASE_RESIDUAL_RADIANS),
            ],
            LIMIT_COLOR.stroke_width(RENDER_SCALE),
        ))?
        .label("phase gate ±0.05 rad")
        .legend(|(x, y)| legend_line(x, y, LIMIT_COLOR));
    chart.draw_series(LineSeries::new(
        [
            (0.0, -MAX_PHASE_RESIDUAL_RADIANS),
            (x_upper, -MAX_PHASE_RESIDUAL_RADIANS),
        ],
        LIMIT_COLOR.stroke_width(RENDER_SCALE),
    ))?;
    draw_markers(
        &mut chart,
        points,
        |point| point.phase_residual_radians,
        CORRECTED_COLOR,
    )?;
    draw_legend(&mut chart)?;
    Ok(())
}

fn draw_added_delay(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    result: &CaseResult,
) -> Result<(), Box<dyn Error>> {
    let points = &result.holdout_points;
    let maximum = result.maximum_added_delay.max(
        points
            .iter()
            .map(|point| point.equalizer_delay)
            .fold(0.0, f64::max),
    );
    let x_upper = result.case.band.upper_frequency_ratio();
    let y_upper = maximum.mul_add(1.08, 0.05);
    let mut chart = ChartBuilder::on(area)
        .caption(
            format!(
                "Added causal delay over selected band — mean {:.3}, maximum {:.3}",
                result.mean_added_delay, result.maximum_added_delay
            ),
            ("sans-serif", scaled(18)),
        )
        .margin(scaled(16))
        .x_label_area_size(scaled(48))
        .y_label_area_size(scaled(72))
        .build_cartesian_2d(0.0_f64..x_upper, 0.0_f64..y_upper)?;
    configure_mesh(
        &mut chart,
        "normalized frequency (ω/ωc)",
        "all-pass delay (ωc τg)",
    )?;
    shade_near_cutoff_region(&mut chart, x_upper, 0.0, y_upper)?;
    chart
        .draw_series(LineSeries::new(
            points
                .iter()
                .map(|point| (point.frequency_ratio, point.equalizer_delay)),
            EQUALIZER_COLOR.stroke_width(scaled(2)),
        ))?
        .label("all-pass delay")
        .legend(|(x, y)| legend_line(x, y, EQUALIZER_COLOR));
    chart
        .draw_series(LineSeries::new(
            [
                (0.0, result.mean_added_delay),
                (x_upper, result.mean_added_delay),
            ],
            LIMIT_COLOR.mix(0.7).stroke_width(RENDER_SCALE),
        ))?
        .label("uniform-band mean")
        .legend(|(x, y)| legend_line(x, y, LIMIT_COLOR));
    draw_markers(
        &mut chart,
        points,
        |point| point.equalizer_delay,
        EQUALIZER_COLOR,
    )?;
    draw_legend(&mut chart)?;
    Ok(())
}

fn configure_mesh<'chart, 'backend: 'chart>(
    chart: &mut ChartContext<
        'chart,
        SVGBackend<'backend>,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    x_description: &str,
    y_description: &str,
) -> Result<(), Box<dyn Error>> {
    chart
        .configure_mesh()
        .x_desc(x_description)
        .y_desc(y_description)
        .x_labels(8)
        .y_labels(9)
        .label_style(("sans-serif", scaled(12)))
        .axis_desc_style(("sans-serif", scaled(15)))
        .axis_style(BLACK.stroke_width(RENDER_SCALE))
        .bold_line_style(RGBColor(174, 181, 190).stroke_width(RENDER_SCALE))
        .light_line_style(RGBColor(225, 229, 235).stroke_width(RENDER_SCALE))
        .draw()?;
    Ok(())
}

fn draw_markers<'chart, 'backend: 'chart, Y>(
    chart: &mut ChartContext<
        'chart,
        SVGBackend<'backend>,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    points: &[SweepPoint],
    y_value: Y,
    color: RGBColor,
) -> Result<(), Box<dyn Error>>
where
    Y: Fn(&SweepPoint) -> f64,
{
    let stride = (points.len() / MARKER_TARGET_COUNT).max(1);
    chart.draw_series(
        points
            .iter()
            .enumerate()
            .filter(|(index, _)| index % stride == 0 || *index + 1 == points.len())
            .map(|(_, point)| {
                Circle::new(
                    (point.frequency_ratio, y_value(point)),
                    scaled(3),
                    color.stroke_width(scaled(1)),
                )
            }),
    )?;
    Ok(())
}

fn shade_near_cutoff_region<'chart, 'backend: 'chart>(
    chart: &mut ChartContext<
        'chart,
        SVGBackend<'backend>,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    x_upper: f64,
    y_lower: f64,
    y_upper: f64,
) -> Result<(), Box<dyn Error>> {
    if x_upper > 0.7 {
        chart.draw_series([Rectangle::new(
            [(0.7, y_lower), (x_upper, y_upper)],
            CORRECTED_COLOR.mix(0.055).filled(),
        )])?;
        chart.draw_series(LineSeries::new(
            [(0.7, y_lower), (0.7, y_upper)],
            LIMIT_COLOR.mix(0.45).stroke_width(RENDER_SCALE),
        ))?;
    }
    Ok(())
}

fn draw_legend<'chart, 'backend: 'chart>(
    chart: &mut ChartContext<
        'chart,
        SVGBackend<'backend>,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
) -> Result<(), Box<dyn Error>> {
    chart
        .configure_series_labels()
        .label_font(("sans-serif", scaled(12)))
        .margin(scaled(5))
        .legend_area_size(scaled(30))
        .background_style(WHITE.mix(0.9))
        .border_style(BLACK.mix(0.35).stroke_width(RENDER_SCALE))
        .position(SeriesLabelPosition::UpperLeft)
        .draw()?;
    Ok(())
}

fn legend_line(x: i32, y: i32, color: RGBColor) -> PathElement<(i32, i32)> {
    PathElement::new(
        [(x, y), (x + i32::try_from(scaled(24)).unwrap(), y)],
        color.stroke_width(scaled(2)),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ssfilt::Response;

    use crate::search::{Band, Case, design};

    use super::{case_file_name, draw};

    #[test]
    fn dashboard_smoke_test_contains_declared_metrics_and_vector_curves() {
        let result = design(Case {
            response: Response::Butterworth,
            response_label: "Butterworth",
            order: 4,
            band: Band::Core,
        })
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "ssfilt-phase-dashboard-{}-{}.svg",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        draw(&path, &result).unwrap();
        crate::svg_smoothing::smooth_svg_curves(&path).unwrap();
        let svg = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("ANALYTIC GATES PASS; NOT STREAMING"));
        assert!(svg.contains("Group delay"));
        assert!(svg.contains("Absolute delay residual"));
        assert!(svg.contains("Residual phase"));
        assert!(svg.contains("Added causal delay"));
        assert!(svg.contains("#2E6FD6"));
        assert!(svg.contains("#E05B4A"));
        assert!(svg.contains("width=\"1600\" height=\"1180\""));
        assert!(svg.contains("<path"));
        assert_eq!(case_file_name(&result), "butterworth-order-4-core.svg");
    }
}
