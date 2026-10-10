use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::render::ResponseData;

pub(super) fn write_csv(path: &Path, responses: &[ResponseData]) -> Result<(), Box<dyn Error>> {
    fs::write(path, csv(responses))?;
    Ok(())
}

fn csv(responses: &[ResponseData]) -> String {
    let mut output = String::from(
        "response,order,frequency_ratio,schedule,precision,input_model,interval_multiplier_pattern,mean_interval_multiplier,minimum_samples_per_cycle,maximum_interval_seconds,warmup_cycles,warmup_seconds,fit_cycles,warmup_updates,fit_updates,fit_samples,settling_windows,settling_relative_gain_delta,settling_phase_delta_radians,phase_resolved,fit_offset,measured_gain,measured_gain_db,analytic_gain_db,gain_error_db,measured_wrapped_phase_radians,measured_phase_radians_nearest_analytic_branch,analytic_phase_radians,phase_error_radians,fit_rms,accepted_steps,rejected_steps,derivative_evaluations,smallest_accepted_step_seconds,largest_accepted_step_seconds\n",
    );
    for response in responses {
        for point in &response.measurements {
            let measured = point.streamed;
            debug_assert_eq!(measured.response, response.spec.response);
            let measured_gain_db = point.measured_gain_db();
            writeln!(
                output,
                "{},{},{:.17},{},f64,linear,{},{:.17},{},{:.17},{:.17},{:.17},{},{},{},{},{},{:.17},{:.17},{},{:.17},{:.17},{:.17},{:.17},{:.17},{:.17},{:.17},{:.17},{:.17},{:.17},{},{},{},{},{}",
                csv_field(response.spec.label),
                measured.order,
                measured.frequency_ratio,
                measured.schedule.label(),
                measured.schedule.pattern_label(),
                measured.schedule.mean_interval_multiplier(),
                measured.samples_per_cycle,
                measured.maximum_interval_seconds,
                measured.warmup_cycles,
                measured.warmup_seconds,
                measured.fit_cycles,
                measured.warmup_update_count,
                measured.fit_update_count,
                measured.fit_sample_count,
                measured.settling_window_count,
                measured.settling_relative_gain_delta,
                measured.settling_phase_delta_radians,
                measured.phase_is_resolved(),
                measured.fit_offset,
                measured.gain,
                measured_gain_db,
                point.analytic_gain_db,
                measured_gain_db - point.analytic_gain_db,
                measured.wrapped_phase_radians,
                point.unwrapped_phase_radians,
                point.analytic_phase_radians,
                point.unwrapped_phase_radians - point.analytic_phase_radians,
                measured.fit_rms,
                measured.diagnostics.accepted_steps,
                measured.diagnostics.rejected_steps,
                measured.diagnostics.derivative_evaluations,
                optional_float(measured.diagnostics.smallest_accepted_step_seconds),
                optional_float(measured.diagnostics.largest_accepted_step_seconds),
            )
            .expect("writing to a String cannot fail");
        }
    }
    output
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn optional_float(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.17}"))
}

#[cfg(test)]
mod tests {
    use plotters::prelude::RGBColor;
    use ssfilt::Response;

    use super::csv;
    use crate::measure::{MeasuredFrequencyPoint, SampleSchedule, measure_low_pass};
    use crate::render::{ResponseData, ResponseSpec};

    #[test]
    fn csv_has_one_data_row_for_every_plotted_measurement() {
        let streamed =
            measure_low_pass::<1>(Response::RepeatedPole, 1.0, SampleSchedule::Uniform, 256)
                .unwrap();
        let data = [ResponseData {
            spec: ResponseSpec {
                label: "response, with comma",
                response: Response::RepeatedPole,
                color: RGBColor(0, 0, 0),
            },
            frequency: Vec::new(),
            measurements: vec![MeasuredFrequencyPoint::new(streamed, -3.0, -0.5)],
            group_delay: Vec::new(),
            step: Vec::new(),
        }];

        let csv = csv(&data);
        assert_eq!(csv.lines().count(), 2);
        assert!(csv.contains("\"response, with comma\""));
        assert!(csv.contains("measured_phase_radians_nearest_analytic_branch"));
        assert!(csv.contains(",uniform,f64,linear,1.0,1.00000000000000000,256,"));
    }
}
