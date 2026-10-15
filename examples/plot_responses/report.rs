use std::error::Error;
use std::fs;
use std::path::Path;

use super::render::ResponseData;

const CSV_HEADERS: &[&str] = &[
    "response",
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
        for point in &response.measurements {
            let measured = point.streamed;
            debug_assert_eq!(measured.response, response.spec.response);
            let measured_gain_db = point.measured_gain_db();
            writer.write_record([
                response.spec.label.to_owned(),
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
                measured.phase_is_resolved().to_string(),
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
            ])?;
        }
    }
    Ok(())
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
            measurements: vec![MeasuredFrequencyPoint::new(streamed, -3.0, -0.5)],
            group_delay: Vec::new(),
            step: Vec::new(),
        }];

        let bytes = csv(&data).unwrap();
        let output = String::from_utf8(bytes.clone()).unwrap();
        assert!(output.starts_with(&format!("{}\n", CSV_HEADERS.join(","))));
        assert!(output.contains("\"response, \"\"quoted\"\"\r\nnext\",1,1.00000000000000000,uniform,f64,linear,1.0,1.00000000000000000,256,"));
        assert!(output.contains(",measured_phase_radians_nearest_analytic_branch,"));
        assert!(output.ends_with(",,\n"));

        let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
        let headers = reader.headers().unwrap().clone();
        let records = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), headers.len());
        assert_eq!(records[0].get(0), Some("response, \"quoted\"\r\nnext"));
        assert_eq!(records[0].get(33), Some(""));
        assert_eq!(records[0].get(34), Some(""));
        assert_eq!(bytes.last(), Some(&b'\n'));
    }
}
