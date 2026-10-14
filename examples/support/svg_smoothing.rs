use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, RESPONSE_STROKES, scaled};

pub(super) fn smooth_svg_curves(
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Plotters maps chart coordinates to integer backend pixels and emits
    // polylines. Replacing only the declared data curves with shape-preserving
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
        if let Some((last_x, y_sum, count)) = collapsed.last_mut()
            && *last_x == x
        {
            *y_sum += f64::from(y);
            *count += 1;
            continue;
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
mod tests {
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
