use std::error::Error;
use std::fs;
use std::path::Path;

use super::render::ResponseData;

const CSV_HEADERS: &[&str] = &[
    "response",
    "variant",
    "measurement_pair_id",
    "equalization_scope",
    "validated_band_edge_ratio",
    "target_normalized_group_delay",
    "mean_added_normalized_group_delay",
    "analytic_normalized_group_delay",
    "normalized_delay_residual_from_target",
    "analytic_magnitude_difference_db",
    "order",
    "frequency_ratio",
    "schedule",
    "precision",
    "input_model",
    "interval_multiplier_pattern",
    "mean_interval_multiplier",
    "minimum_samples_per_cycle",
    "maximum_interval_seconds",
    "warmup_cycles",
    "warmup_seconds",
    "fit_cycles",
    "warmup_updates",
    "fit_updates",
    "fit_samples",
    "settling_windows",
    "settling_relative_gain_delta",
    "settling_phase_delta_radians",
    "phase_resolved",
    "fit_offset",
    "measured_gain",
    "measured_gain_db",
    "analytic_gain_db",
    "gain_error_db",
    "measured_wrapped_phase_radians",
    "measured_phase_radians_nearest_analytic_branch",
    "analytic_phase_radians",
    "phase_error_radians",
    "fit_rms",
    "accepted_steps",
    "rejected_steps",
    "derivative_evaluations",
    "smallest_accepted_step_seconds",
    "largest_accepted_step_seconds",
];

pub(super) fn write_csv(
    path: &Path,
    responses: &[ResponseData],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    fs::write(path, csv(responses)?)?;
    Ok(())
}

fn csv(responses: &[ResponseData]) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let mut output = Vec::new();
    {
        let mut writer = csv_writer(&mut output);
        write_records(&mut writer, responses)?;
        writer.flush()?;
    }
    Ok(output)
}

fn csv_writer<W: std::io::Write>(output: W) -> csv::Writer<W> {
    csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(output)
}

fn write_records<W: std::io::Write>(
    writer: &mut csv::Writer<W>,
    responses: &[ResponseData],
) -> csv::Result<()> {
    writer.write_record(CSV_HEADERS.iter().copied())?;
    for response in responses {
        if let Some(equalized) = &response.phase_equalized
            && equalized.measurements.len() != response.measurements.len()
        {
            return Err(csv::Error::from(std::io::Error::other(format!(
                "{} source/equalized measurement counts differ: {} versus {}",
                response.spec.label,
                response.measurements.len(),
                equalized.measurements.len()
            ))));
        }
        for point in &response.measurements {
            write_point(writer, response, "source", point, None, None)?;
        }
        if let Some(equalized) = &response.phase_equalized {
            for (source, point) in response.measurements.iter().zip(&equalized.measurements) {
                write_point(
                    writer,
                    response,
                    "phase-equalized",
                    point,
                    Some(equalized),
                    Some(source.analytic_gain_db),
                )?;
            }
        }
    }
    Ok(())
}

fn write_point<W: std::io::Write>(
    writer: &mut csv::Writer<W>,
    response: &ResponseData,
    variant: &str,
    point: &crate::measure::MeasuredFrequencyPoint,
    equalized: Option<&crate::render::PhaseEqualizedData>,
    source_analytic_gain_db: Option<f64>,
) -> csv::Result<()> {
    let measured = point.streamed;
    debug_assert_eq!(measured.response, response.spec.response);
    let measured_gain_db = point.measured_gain_db();
    let metadata = equalized.or(response.phase_equalized.as_ref());
    let pair_id = metadata.map_or_else(String::new, |_| {
        format!(
            "o{}-r{:.17}-{}",
            measured.order,
            measured.frequency_ratio,
            measured.schedule.label()
        )
    });
    let scope = match equalized {
        Some(data) if measured.frequency_ratio <= data.validated_band_edge_ratio => "validated",
        Some(_) => "diagnostic",
        None => "not-applicable",
    };
    let delay_residual =
        equalized.map(|data| point.analytic_normalized_group_delay - data.target_normalized_delay);
    let magnitude_difference =
        source_analytic_gain_db.map(|source_gain| point.analytic_gain_db - source_gain);
    let mut record = vec![
        response.spec.label.to_owned(),
        variant.to_owned(),
        pair_id,
        scope.to_owned(),
        metadata.map_or_else(String::new, |data| {
            format!("{:.17}", data.validated_band_edge_ratio)
        }),
        metadata.map_or_else(String::new, |data| {
            format!("{:.17}", data.target_normalized_delay)
        }),
        metadata.map_or_else(String::new, |data| {
            format!("{:.17}", data.mean_added_normalized_delay)
        }),
        format!("{:.17}", point.analytic_normalized_group_delay),
        delay_residual.map_or_else(String::new, |value| format!("{value:.17}")),
        magnitude_difference.map_or_else(String::new, |value| format!("{value:.17}")),
        measured.order.to_string(),
        format!("{:.17}", measured.frequency_ratio),
        measured.schedule.label().to_owned(),
        "f64".to_owned(),
        "linear".to_owned(),
        measured.schedule.pattern_label().to_owned(),
        format!("{:.17}", measured.schedule.mean_interval_multiplier()),
        measured.samples_per_cycle.to_string(),
        format!("{:.17}", measured.maximum_interval_seconds),
        format!("{:.17}", measured.warmup_cycles),
        format!("{:.17}", measured.warmup_seconds),
        measured.fit_cycles.to_string(),
        measured.warmup_update_count.to_string(),
        measured.fit_update_count.to_string(),
        measured.fit_sample_count.to_string(),
        measured.settling_window_count.to_string(),
        format!("{:.17}", measured.settling_relative_gain_delta),
        format!("{:.17}", measured.settling_phase_delta_radians),
        point.phase_is_resolved().to_string(),
        format!("{:.17}", measured.fit_offset),
        format!("{:.17}", measured.gain),
        format!("{measured_gain_db:.17}"),
        format!("{:.17}", point.analytic_gain_db),
        format!("{:.17}", measured_gain_db - point.analytic_gain_db),
        format!("{:.17}", measured.wrapped_phase_radians),
        format!("{:.17}", point.unwrapped_phase_radians),
        format!("{:.17}", point.analytic_phase_radians),
        format!(
            "{:.17}",
            point.unwrapped_phase_radians - point.analytic_phase_radians
        ),
        format!("{:.17}", measured.fit_rms),
        measured.diagnostics.accepted_steps.to_string(),
        measured.diagnostics.rejected_steps.to_string(),
        measured.diagnostics.derivative_evaluations.to_string(),
        optional_float(measured.diagnostics.smallest_accepted_step_seconds),
        optional_float(measured.diagnostics.largest_accepted_step_seconds),
    ];
    debug_assert_eq!(record.len(), CSV_HEADERS.len());
    writer.write_record(record.drain(..))
}

fn optional_float(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.17}"))
}

#[cfg(test)]
mod tests {
    use plotters::prelude::RGBColor;
    use ssfilt::Response;

    use super::{CSV_HEADERS, csv};
    use crate::measure::{MeasuredFrequencyPoint, SampleSchedule, measure_low_pass};
    use crate::render::{ResponseData, ResponseSpec};

    #[test]
    fn csv_has_one_data_row_for_every_plotted_measurement() {
        let streamed =
            measure_low_pass::<1>(Response::RepeatedPole, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let mut streamed = streamed;
        streamed.diagnostics.smallest_accepted_step_seconds = None;
        streamed.diagnostics.largest_accepted_step_seconds = None;
        let data = [ResponseData {
            spec: ResponseSpec {
                label: "response, \"quoted\"\r\nnext",
                response: Response::RepeatedPole,
                color: RGBColor(0, 0, 0),
            },
            frequency: Vec::new(),
            measurements: vec![MeasuredFrequencyPoint::new(streamed, -3.0, -0.5, 1.25)],
            group_delay: Vec::new(),
            step: Vec::new(),
            phase_equalized: None,
        }];

        let bytes = csv(&data).unwrap();
        let output = String::from_utf8(bytes.clone()).unwrap();
        assert!(output.starts_with(&format!("{}\n", CSV_HEADERS.join(","))));
        assert!(output.contains(
            "\"response, \"\"quoted\"\"\r\nnext\",source,,not-applicable,,,,1.25000000000000000,,,1,1.00000000000000000,uniform,f64,linear,1.0,1.00000000000000000,256,"
        ));
        assert!(output.contains(",measured_phase_radians_nearest_analytic_branch,"));
        assert!(output.ends_with(",,\n"));

        let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
        let headers = reader.headers().unwrap().clone();
        let records = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), headers.len());
        assert_eq!(records[0].get(0), Some("response, \"quoted\"\r\nnext"));
        assert_eq!(records[0].get(42), Some(""));
        assert_eq!(records[0].get(43), Some(""));
        assert_eq!(bytes.last(), Some(&b'\n'));
    }

    #[test]
    fn equalized_rows_disclose_scope_and_share_pair_identity_with_the_source() {
        use crate::render::PhaseEqualizedData;

        let streamed =
            measure_low_pass::<1>(Response::Butterworth, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let source = MeasuredFrequencyPoint::new(streamed, -3.0, -0.5, 1.0);
        let equalized = MeasuredFrequencyPoint::new(streamed, -3.0, -1.0, 1.1);
        let data = [ResponseData {
            spec: ResponseSpec {
                label: "Butterworth",
                response: Response::Butterworth,
                color: RGBColor(0, 0, 0),
            },
            frequency: Vec::new(),
            measurements: vec![source],
            group_delay: Vec::new(),
            step: Vec::new(),
            phase_equalized: Some(PhaseEqualizedData {
                frequency: Vec::new(),
                measurements: vec![equalized],
                step: Vec::new(),
                validated_band_edge_ratio: 0.7,
                target_normalized_delay: 1.05,
                mean_added_normalized_delay: 0.5,
            }),
        }];

        let bytes = csv(&data).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
        let headers = reader.headers().unwrap().clone();
        let records = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        let index = |name: &str| headers.iter().position(|header| header == name).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get(index("variant")), Some("source"));
        assert_eq!(records[1].get(index("variant")), Some("phase-equalized"));
        assert_eq!(
            records[0].get(index("measurement_pair_id")),
            records[1].get(index("measurement_pair_id"))
        );
        assert_eq!(
            records[1].get(index("equalization_scope")),
            Some("diagnostic")
        );
        assert_eq!(
            records[1].get(index("analytic_magnitude_difference_db")),
            Some("0.00000000000000000")
        );
    }

    #[test]
    fn csv_keeps_below_floor_rows_but_marks_their_phase_unresolved() {
        let streamed =
            measure_low_pass::<1>(Response::Butterworth, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let data = [ResponseData {
            spec: ResponseSpec {
                label: "below-floor",
                response: Response::Butterworth,
                color: RGBColor(0, 0, 0),
            },
            frequency: Vec::new(),
            measurements: vec![MeasuredFrequencyPoint::new(streamed, -150.0, -2.0, 0.1)],
            group_delay: Vec::new(),
            step: Vec::new(),
            phase_equalized: None,
        }];

        let bytes = csv(&data).unwrap();
        let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
        let headers = reader.headers().unwrap().clone();
        let record = reader.records().next().unwrap().unwrap();
        let phase_index = headers
            .iter()
            .position(|header| header == "phase_resolved")
            .unwrap();
        assert_eq!(record.get(phase_index), Some("false"));
    }

    #[test]
    fn csv_rejects_unpaired_source_and_equalized_measurement_counts() {
        use crate::render::PhaseEqualizedData;

        let streamed =
            measure_low_pass::<1>(Response::Butterworth, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let data = [ResponseData {
            spec: ResponseSpec {
                label: "Butterworth",
                response: Response::Butterworth,
                color: RGBColor(0, 0, 0),
            },
            frequency: Vec::new(),
            measurements: vec![MeasuredFrequencyPoint::new(streamed, -3.0, -0.5, 1.0)],
            group_delay: Vec::new(),
            step: Vec::new(),
            phase_equalized: Some(PhaseEqualizedData {
                frequency: Vec::new(),
                measurements: Vec::new(),
                step: Vec::new(),
                validated_band_edge_ratio: 0.7,
                target_normalized_delay: 1.05,
                mean_added_normalized_delay: 0.5,
            }),
        }];

        let error = csv(&data).unwrap_err().to_string();
        assert!(error.contains("source/equalized measurement counts differ"));
    }
}
