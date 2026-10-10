use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use crate::search::{
    COARSE_POINTS_PER_AXIS, CROSSCHECK_INTERVAL_COUNT, FIT_POINT_COUNT, HOLDOUT_INTERVAL_COUNT,
    LOCAL_GRID_POINTS_PER_AXIS, MAX_REFINEMENT_MOVES_PER_LEVEL, REFINEMENT_LEVELS,
    REFINEMENT_SEED_COUNT,
};
use crate::search::{CaseResult, GateResults};

const SUMMARY_HEADERS: &[&str] = &[
    "response",
    "order",
    "band",
    "upper_frequency_ratio",
    "equalizer_sigma",
    "equalizer_eta",
    "natural_frequency",
    "section_q",
    "numerator_s2",
    "numerator_s1",
    "numerator_s0",
    "denominator_s2",
    "denominator_s1",
    "denominator_s0",
    "source_target_delay",
    "source_peak_delay_residual",
    "source_rms_delay_residual",
    "source_max_phase_residual_radians",
    "corrected_target_delay",
    "corrected_peak_delay_residual",
    "corrected_rms_delay_residual",
    "corrected_max_phase_residual_radians",
    "f32_corrected_target_delay",
    "f32_corrected_peak_delay_residual",
    "f32_corrected_rms_delay_residual",
    "f32_corrected_max_phase_residual_radians",
    "mean_added_delay",
    "maximum_added_delay",
    "f32_mean_added_delay",
    "f32_maximum_added_delay",
    "maximum_f64_magnitude_change",
    "maximum_f32_magnitude_change",
    "fit_grid_kind",
    "fit_point_count",
    "coarse_points_per_axis",
    "local_points_per_axis",
    "refinement_seed_count",
    "refinement_levels",
    "maximum_refinement_moves_per_level",
    "fit_evaluations",
    "holdout_grid_kind",
    "holdout_interval_count",
    "holdout_nominal_point_count",
    "holdout_stored_point_count",
    "crosscheck_grid_kind",
    "crosscheck_interval_count",
    "crosscheck_nominal_point_count",
    "crosscheck_evaluated_point_count",
    "crosscheck_target_delay",
    "crosscheck_peak_residual",
    "crosscheck_rms_residual",
    "crosscheck_max_phase_residual_radians",
    "gate_absolute_peak",
    "gate_relative_peak",
    "gate_phase",
    "gate_mean_added_delay",
    "gate_f64_magnitude",
    "gate_f32_magnitude",
    "gate_f64_bounds",
    "gate_f32_bounds",
    "all_gates_pass",
];

const SWEEP_HEADERS: &[&str] = &[
    "response",
    "order",
    "band",
    "sweep_index",
    "sweep_point_count",
    "frequency_ratio",
    "source_delay",
    "source_delay_residual",
    "equalizer_delay",
    "total_delay",
    "corrected_delay_residual",
    "source_phase_radians",
    "equalizer_phase_radians",
    "total_phase_radians",
    "phase_residual_radians",
    "source_target_delay",
    "corrected_target_delay",
];

const GATE_DEFINITIONS: &[(&str, &str)] = &[
    (
        "absolute_peak",
        "f64 and f32-coefficient corrected peak delay residual <= 0.10",
    ),
    (
        "relative_peak",
        "f64 and f32-coefficient corrected peak delay residual <= 0.25 * source peak residual",
    ),
    (
        "phase",
        "f64 and f32-coefficient maximum corrected phase residual <= 0.05 radians",
    ),
    (
        "mean_added_delay",
        "f64 and f32-coefficient mean added delay <= 5.0",
    ),
    ("f64_magnitude", "maximum f64 magnitude change <= 1e-12"),
    ("f32_magnitude", "maximum f32 magnitude change <= 1e-5"),
    (
        "f64_bounds",
        "f64 section has sigma >= 0.2, eta >= 0, Q <= 2, and wn <= 4",
    ),
    (
        "f32_bounds",
        "f32-rounded section has sigma >= 0.2, eta >= 0, Q <= 2, and wn <= 4",
    ),
];

pub(super) fn write_reports(output_dir: &Path, results: &[CaseResult]) -> io::Result<()> {
    fs::create_dir_all(output_dir)?;
    fs::write(output_dir.join("summary.csv"), summary_csv(results))?;
    fs::write(output_dir.join("sweep.csv"), sweep_csv(results))?;
    fs::write(output_dir.join("summary.md"), summary_markdown(results))?;
    Ok(())
}

pub(super) fn format_float(value: f64) -> String {
    // Seventeen significant decimal digits are sufficient to round-trip f64.
    format!("{value:.16e}")
}

pub(super) fn failed_gate_names(gates: GateResults) -> Vec<&'static str> {
    gate_results(gates)
        .into_iter()
        .filter_map(|(name, passed)| (!passed).then_some(name))
        .collect()
}

fn summary_csv(results: &[CaseResult]) -> String {
    let mut output = String::new();
    append_record(
        &mut output,
        SUMMARY_HEADERS.iter().map(|header| (*header).to_owned()),
    );
    for result in results {
        let candidate = result.candidate;
        let natural_frequency = candidate.natural_frequency();
        let denominator_a2 = natural_frequency * natural_frequency;
        let mut fields = vec![
            result.case.response_label.to_owned(),
            result.case.order.to_string(),
            result.case.band.label().to_owned(),
            float(result.case.band.upper_frequency_ratio()),
            float(candidate.sigma),
            float(candidate.eta),
            float(natural_frequency),
            float(candidate.q()),
            float(1.0),
            float(-2.0 * candidate.sigma),
            float(denominator_a2),
            float(1.0),
            float(2.0 * candidate.sigma),
            float(denominator_a2),
            float(result.source_metrics.target_delay),
            float(result.source_metrics.peak_residual),
            float(result.source_metrics.rms_residual),
            float(result.source_metrics.maximum_phase_residual_radians),
            float(result.corrected_metrics.target_delay),
            float(result.corrected_metrics.peak_residual),
            float(result.corrected_metrics.rms_residual),
            float(result.corrected_metrics.maximum_phase_residual_radians),
            float(result.f32_corrected_metrics.target_delay),
            float(result.f32_corrected_metrics.peak_residual),
            float(result.f32_corrected_metrics.rms_residual),
            float(result.f32_corrected_metrics.maximum_phase_residual_radians),
            float(result.mean_added_delay),
            float(result.maximum_added_delay),
            float(result.f32_mean_added_delay),
            float(result.f32_maximum_added_delay),
            float(result.maximum_f64_magnitude_change),
            float(result.maximum_f32_magnitude_change),
            "midpoint".to_owned(),
            FIT_POINT_COUNT.to_string(),
            COARSE_POINTS_PER_AXIS.to_string(),
            LOCAL_GRID_POINTS_PER_AXIS.to_string(),
            REFINEMENT_SEED_COUNT.to_string(),
            REFINEMENT_LEVELS.to_string(),
            MAX_REFINEMENT_MOVES_PER_LEVEL.to_string(),
            result.fit_evaluations.to_string(),
            "endpoint-inclusive uniform plus candidate pole features".to_owned(),
            HOLDOUT_INTERVAL_COUNT.to_string(),
            (HOLDOUT_INTERVAL_COUNT + 1).to_string(),
            result.holdout_points.len().to_string(),
            "endpoint-inclusive uniform plus candidate pole features".to_owned(),
            CROSSCHECK_INTERVAL_COUNT.to_string(),
            (CROSSCHECK_INTERVAL_COUNT + 1).to_string(),
            result.crosscheck_evaluated_point_count.to_string(),
            float(result.crosscheck_metrics.target_delay),
            float(result.crosscheck_metrics.peak_residual),
            float(result.crosscheck_metrics.rms_residual),
            float(result.crosscheck_metrics.maximum_phase_residual_radians),
        ];
        fields.extend(
            gate_results(result.gates)
                .into_iter()
                .map(|(_, passed)| passed.to_string()),
        );
        fields.push(result.gates.passes().to_string());
        debug_assert_eq!(fields.len(), SUMMARY_HEADERS.len());
        append_record(&mut output, fields);
    }
    output
}

fn sweep_csv(results: &[CaseResult]) -> String {
    let mut output = String::new();
    append_record(
        &mut output,
        SWEEP_HEADERS.iter().map(|header| (*header).to_owned()),
    );
    for result in results {
        let point_count = result.holdout_points.len();
        for (index, point) in result.holdout_points.iter().enumerate() {
            append_record(
                &mut output,
                [
                    result.case.response_label.to_owned(),
                    result.case.order.to_string(),
                    result.case.band.label().to_owned(),
                    index.to_string(),
                    point_count.to_string(),
                    float(point.frequency_ratio),
                    float(point.source_delay),
                    float(point.source_delay - result.source_metrics.target_delay),
                    float(point.equalizer_delay),
                    float(point.total_delay),
                    float(point.total_delay - result.corrected_metrics.target_delay),
                    float(point.source_phase_radians),
                    float(point.equalizer_phase_radians),
                    float(point.total_phase_radians),
                    float(point.phase_residual_radians),
                    float(result.source_metrics.target_delay),
                    float(result.corrected_metrics.target_delay),
                ],
            );
        }
    }
    output
}

fn summary_markdown(results: &[CaseResult]) -> String {
    let mut output = String::new();
    output.push_str(
        "# Phase design search report\n\nThis is an analytic feasibility report, not evidence from a streaming equalizer implementation. It uses the stored search results. `sweep.csv` contains every holdout point from each result, including its actual frequency, delay, and phase values. Delays and frequencies are normalized by the source low-pass cutoff.\n\n",
    );
    output.push_str("## Gate definitions\n\n| Gate | Criterion |\n| --- | --- |\n");
    for (name, criterion) in GATE_DEFINITIONS {
        let _ = writeln!(output, "| `{name}` | {criterion} |");
    }

    output.push_str(
        "\n## Results\n\n| Order | Band | Analytic status | Source peak | Corrected peak | Corrected RMS | Corrected max phase (rad) | Mean added delay | Maximum added delay |\n| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for result in results {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            result.case.order,
            result.case.band.label(),
            if result.gates.passes() {
                "GATES PASS — NOT STREAMING"
            } else {
                "GATES REJECT — NOT STREAMING"
            },
            float(result.source_metrics.peak_residual),
            float(result.corrected_metrics.peak_residual),
            float(result.corrected_metrics.rms_residual),
            float(result.corrected_metrics.maximum_phase_residual_radians),
            float(result.mean_added_delay),
            float(result.maximum_added_delay),
        );
    }

    for result in results {
        append_case_markdown(&mut output, result);
    }
    output
}

fn append_case_markdown(output: &mut String, result: &CaseResult) {
    let _ = writeln!(
        output,
        "\n## {} order {} / {} band\n",
        result.case.response_label,
        result.case.order,
        result.case.band.label(),
    );
    append_case_metrics(output, result);
    append_case_gates(output, result);
}

fn append_case_metrics(output: &mut String, result: &CaseResult) {
    output.push_str("| Measurement | Value |\n| --- | ---: |\n");
    append_metric_rows(output, equalizer_metric_rows(result));
    append_metric_rows(output, response_metric_rows(result));
    append_metric_rows(output, search_metric_rows(result));
}

fn equalizer_metric_rows(result: &CaseResult) -> Vec<(&'static str, String)> {
    let candidate = result.candidate;
    let natural_frequency = candidate.natural_frequency();
    let denominator_a2 = natural_frequency * natural_frequency;
    vec![
        (
            "Upper frequency ratio",
            float(result.case.band.upper_frequency_ratio()),
        ),
        ("Equalizer sigma", float(candidate.sigma)),
        ("Equalizer eta", float(candidate.eta)),
        ("Equalizer natural frequency wn", float(natural_frequency)),
        ("Equalizer section Q", float(candidate.q())),
        ("Numerator s^2 coefficient", float(1.0)),
        ("Numerator s coefficient", float(-2.0 * candidate.sigma)),
        ("Numerator constant coefficient", float(denominator_a2)),
        ("Denominator s^2 coefficient", float(1.0)),
        ("Denominator s coefficient", float(2.0 * candidate.sigma)),
        ("Denominator constant coefficient", float(denominator_a2)),
    ]
}

fn response_metric_rows(result: &CaseResult) -> Vec<(&'static str, String)> {
    vec![
        (
            "Source target delay",
            float(result.source_metrics.target_delay),
        ),
        (
            "Source peak delay residual",
            float(result.source_metrics.peak_residual),
        ),
        (
            "Source RMS delay residual",
            float(result.source_metrics.rms_residual),
        ),
        (
            "Source maximum phase residual (radians)",
            float(result.source_metrics.maximum_phase_residual_radians),
        ),
        (
            "Corrected target delay",
            float(result.corrected_metrics.target_delay),
        ),
        (
            "Corrected peak delay residual",
            float(result.corrected_metrics.peak_residual),
        ),
        (
            "Corrected RMS delay residual",
            float(result.corrected_metrics.rms_residual),
        ),
        (
            "Corrected maximum phase residual (radians)",
            float(result.corrected_metrics.maximum_phase_residual_radians),
        ),
        (
            "f32-coefficient corrected target delay",
            float(result.f32_corrected_metrics.target_delay),
        ),
        (
            "f32-coefficient corrected peak delay residual",
            float(result.f32_corrected_metrics.peak_residual),
        ),
        (
            "f32-coefficient corrected RMS delay residual",
            float(result.f32_corrected_metrics.rms_residual),
        ),
        (
            "f32-coefficient corrected maximum phase residual (radians)",
            float(result.f32_corrected_metrics.maximum_phase_residual_radians),
        ),
        ("Mean added delay", float(result.mean_added_delay)),
        ("Maximum added delay", float(result.maximum_added_delay)),
        (
            "f32-coefficient mean added delay",
            float(result.f32_mean_added_delay),
        ),
        (
            "f32-coefficient maximum added delay",
            float(result.f32_maximum_added_delay),
        ),
        (
            "Maximum f64 magnitude change",
            float(result.maximum_f64_magnitude_change),
        ),
        (
            "Maximum f32 magnitude change",
            float(result.maximum_f32_magnitude_change),
        ),
    ]
}

fn search_metric_rows(result: &CaseResult) -> Vec<(&'static str, String)> {
    vec![
        (
            "Fit/search configuration",
            format!(
                "{FIT_POINT_COUNT} midpoint frequencies; {COARSE_POINTS_PER_AXIS}x{COARSE_POINTS_PER_AXIS} coarse; {LOCAL_GRID_POINTS_PER_AXIS}x{LOCAL_GRID_POINTS_PER_AXIS} local; {REFINEMENT_SEED_COUNT} seeds; {REFINEMENT_LEVELS} levels; at most {MAX_REFINEMENT_MOVES_PER_LEVEL} moves per level"
            ),
        ),
        ("Fit evaluations", result.fit_evaluations.to_string()),
        (
            "Holdout grid",
            format!(
                "{} stored points; {} uniform intervals plus candidate pole features",
                result.holdout_points.len(),
                HOLDOUT_INTERVAL_COUNT
            ),
        ),
        (
            "Crosscheck grid",
            format!(
                "{} evaluated points; {} uniform intervals ({} endpoint-inclusive base points) plus candidate pole features",
                result.crosscheck_evaluated_point_count,
                CROSSCHECK_INTERVAL_COUNT,
                CROSSCHECK_INTERVAL_COUNT + 1
            ),
        ),
        (
            "Crosscheck target delay",
            float(result.crosscheck_metrics.target_delay),
        ),
        (
            "Crosscheck peak delay residual",
            float(result.crosscheck_metrics.peak_residual),
        ),
        (
            "Crosscheck RMS delay residual",
            float(result.crosscheck_metrics.rms_residual),
        ),
        (
            "Crosscheck maximum phase residual (radians)",
            float(result.crosscheck_metrics.maximum_phase_residual_radians),
        ),
    ]
}

fn append_metric_rows(
    output: &mut String,
    metrics: impl IntoIterator<Item = (&'static str, String)>,
) {
    for (measurement, value) in metrics {
        let _ = writeln!(output, "| {measurement} | {value} |");
    }
}

fn append_case_gates(output: &mut String, result: &CaseResult) {
    output.push_str("\n### Gate results\n\n| Gate | Result |\n| --- | --- |\n");
    for (name, passed) in gate_results(result.gates) {
        let _ = writeln!(
            output,
            "| `{name}` | {} |",
            if passed { "PASS" } else { "FAIL" }
        );
    }
    let _ = writeln!(
        output,
        "| `all_gates` | {} |",
        if result.gates.passes() {
            "PASS"
        } else {
            "FAIL"
        },
    );
}

fn gate_results(gates: GateResults) -> [(&'static str, bool); 8] {
    [
        ("absolute_peak", gates.absolute_peak),
        ("relative_peak", gates.relative_peak),
        ("phase", gates.phase),
        ("mean_added_delay", gates.mean_added_delay),
        ("f64_magnitude", gates.f64_magnitude),
        ("f32_magnitude", gates.f32_magnitude),
        ("f64_bounds", gates.f64_bounds),
        ("f32_bounds", gates.f32_bounds),
    ]
}

fn append_record(output: &mut String, fields: impl IntoIterator<Item = String>) {
    for (index, field) in fields.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        append_csv_field(output, &field);
    }
    output.push('\n');
}

fn append_csv_field(output: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        output.push('"');
        for character in value.chars() {
            if character == '"' {
                output.push('"');
            }
            output.push(character);
        }
        output.push('"');
    } else {
        output.push_str(value);
    }
}

fn float(value: f64) -> String {
    format_float(value)
}

#[cfg(test)]
mod tests {
    use crate::search::{Band, Candidate, Case, CaseResult, DelayMetrics, GateResults, SweepPoint};
    use ssfilt::Response;

    use super::{
        SUMMARY_HEADERS, SWEEP_HEADERS, append_csv_field, format_float, summary_csv,
        summary_markdown, sweep_csv,
    };

    fn sample_result() -> CaseResult {
        CaseResult {
            case: Case {
                response: Response::Butterworth,
                response_label: "Butterworth",
                order: 4,
                band: Band::Core,
            },
            candidate: Candidate {
                sigma: 0.25,
                eta: 0.75,
            },
            source_metrics: DelayMetrics {
                target_delay: 1.1,
                peak_residual: 0.12,
                rms_residual: 0.07,
                maximum_phase_residual_radians: 0.08,
            },
            corrected_metrics: DelayMetrics {
                target_delay: 2.2,
                peak_residual: 0.01,
                rms_residual: 0.006,
                maximum_phase_residual_radians: 0.02,
            },
            f32_corrected_metrics: DelayMetrics {
                target_delay: 2.21,
                peak_residual: 0.011,
                rms_residual: 0.006_1,
                maximum_phase_residual_radians: 0.021,
            },
            mean_added_delay: 1.7,
            maximum_added_delay: 2.8,
            f32_mean_added_delay: 1.71,
            f32_maximum_added_delay: 2.81,
            maximum_f64_magnitude_change: 1.0e-15,
            maximum_f32_magnitude_change: 1.0e-7,
            gates: GateResults {
                absolute_peak: true,
                relative_peak: true,
                phase: true,
                mean_added_delay: true,
                f64_magnitude: true,
                f32_magnitude: true,
                f64_bounds: false,
                f32_bounds: true,
            },
            fit_evaluations: 12_345,
            holdout_points: vec![
                SweepPoint {
                    frequency_ratio: 0.0,
                    source_delay: 0.5,
                    equalizer_delay: 1.0,
                    total_delay: 1.5,
                    source_phase_radians: 0.0,
                    equalizer_phase_radians: 0.0,
                    total_phase_radians: 0.0,
                    phase_residual_radians: 0.0,
                },
                SweepPoint {
                    frequency_ratio: 0.7,
                    source_delay: 0.4,
                    equalizer_delay: 0.9,
                    total_delay: 1.3,
                    source_phase_radians: -0.8,
                    equalizer_phase_radians: -1.1,
                    total_phase_radians: -1.9,
                    phase_residual_radians: -0.36,
                },
            ],
            crosscheck_evaluated_point_count: 16_388,
            crosscheck_metrics: DelayMetrics {
                target_delay: 2.200_001,
                peak_residual: 0.010_001,
                rms_residual: 0.006_001,
                maximum_phase_residual_radians: 0.020_001,
            },
        }
    }

    fn cells(record: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut characters = record.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '"' if quoted && characters.peek() == Some(&'"') => {
                    current.push('"');
                    characters.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => {
                    fields.push(std::mem::take(&mut current));
                }
                _ => current.push(character),
            }
        }
        fields.push(current);
        fields
    }

    #[test]
    fn summary_csv_maps_metrics_coefficients_metadata_and_each_gate() {
        let csv = summary_csv(&[sample_result()]);
        let mut lines = csv.lines();
        let headers = cells(lines.next().unwrap());
        let values = cells(lines.next().unwrap());
        assert_eq!(headers, SUMMARY_HEADERS);
        assert_eq!(headers.len(), values.len());
        let get =
            |name: &str| values[headers.iter().position(|header| header == name).unwrap()].as_str();

        assert_eq!(get("response"), "Butterworth");
        assert_eq!(get("order"), "4");
        assert_eq!(get("band"), "core");
        assert_eq!(get("equalizer_sigma"), format_float(0.25));
        assert_eq!(get("equalizer_eta"), format_float(0.75));
        assert_eq!(get("natural_frequency"), format_float(0.25_f64.hypot(0.75)));
        assert_eq!(get("section_q"), format_float(0.25_f64.hypot(0.75) / 0.5));
        assert_eq!(get("numerator_s1"), format_float(-0.5));
        assert_eq!(get("denominator_s1"), format_float(0.5));
        assert_eq!(get("source_peak_delay_residual"), format_float(0.12));
        assert_eq!(get("corrected_rms_delay_residual"), format_float(0.006));
        assert_eq!(get("source_max_phase_residual_radians"), format_float(0.08));
        assert_eq!(
            get("corrected_max_phase_residual_radians"),
            format_float(0.02)
        );
        assert_eq!(
            get("f32_corrected_peak_delay_residual"),
            format_float(0.011)
        );
        assert_eq!(
            get("f32_corrected_max_phase_residual_radians"),
            format_float(0.021)
        );
        assert_eq!(get("mean_added_delay"), format_float(1.7));
        assert_eq!(get("maximum_added_delay"), format_float(2.8));
        assert_eq!(get("f32_mean_added_delay"), format_float(1.71));
        assert_eq!(get("f32_maximum_added_delay"), format_float(2.81));
        assert_eq!(get("fit_grid_kind"), "midpoint");
        assert_eq!(get("fit_point_count"), "4096");
        assert_eq!(get("coarse_points_per_axis"), "65");
        assert_eq!(get("local_points_per_axis"), "33");
        assert_eq!(get("refinement_seed_count"), "8");
        assert_eq!(get("refinement_levels"), "18");
        assert_eq!(get("maximum_refinement_moves_per_level"), "16");
        assert_eq!(get("fit_evaluations"), "12345");
        assert_eq!(get("holdout_stored_point_count"), "2");
        assert_eq!(get("crosscheck_interval_count"), "16386");
        assert_eq!(get("crosscheck_evaluated_point_count"), "16388");
        assert_eq!(get("crosscheck_target_delay"), format_float(2.200_001));
        assert_eq!(get("crosscheck_peak_residual"), format_float(0.010_001));
        assert_eq!(get("crosscheck_rms_residual"), format_float(0.006_001));
        assert_eq!(
            get("crosscheck_max_phase_residual_radians"),
            format_float(0.020_001)
        );
        assert_eq!(get("gate_absolute_peak"), "true");
        assert_eq!(get("gate_f64_bounds"), "false");
        assert_eq!(get("gate_f32_bounds"), "true");
        assert_eq!(get("all_gates_pass"), "false");
    }

    #[test]
    fn sweep_csv_uses_the_stored_point_values() {
        let csv = sweep_csv(&[sample_result()]);
        let mut lines = csv.lines();
        let headers = cells(lines.next().unwrap());
        let _first_point = lines.next().unwrap();
        let second = cells(lines.next().unwrap());
        assert_eq!(headers, SWEEP_HEADERS);
        assert_eq!(headers.len(), second.len());
        let get =
            |name: &str| second[headers.iter().position(|header| header == name).unwrap()].as_str();

        assert_eq!(get("sweep_index"), "1");
        assert_eq!(get("sweep_point_count"), "2");
        assert_eq!(get("frequency_ratio"), format_float(0.7));
        assert_eq!(get("source_delay"), format_float(0.4));
        assert_eq!(get("equalizer_delay"), format_float(0.9));
        assert_eq!(get("total_delay"), format_float(1.3));
        assert_eq!(get("source_phase_radians"), format_float(-0.8));
        assert_eq!(get("equalizer_phase_radians"), format_float(-1.1));
        assert_eq!(get("phase_residual_radians"), format_float(-0.36));
    }

    #[test]
    fn markdown_contains_report_metrics_and_each_gate_result() {
        let markdown = summary_markdown(&[sample_result()]);
        assert!(markdown.contains("Numerator s coefficient"));
        assert!(markdown.contains("Source RMS delay residual"));
        assert!(markdown.contains("Corrected maximum phase residual"));
        assert!(markdown.contains("f32-coefficient corrected maximum phase residual"));
        assert!(markdown.contains("12345"));
        assert!(markdown.contains("| `f64_bounds` | FAIL |"));
        assert!(markdown.contains("| `f32_bounds` | PASS |"));
        assert!(markdown.contains("sweep.csv` contains every holdout point"));
    }

    #[test]
    fn csv_escapes_delimiters_quotes_and_line_breaks_and_uses_round_trip_precision() {
        let mut escaped = String::new();
        append_csv_field(&mut escaped, "Band, \"core\"\nnext");
        assert_eq!(escaped, "\"Band, \"\"core\"\"\nnext\"");
        assert_eq!(
            format_float(1.234_567_890_123_456_7),
            "1.2345678901234567e0"
        );
    }
}
