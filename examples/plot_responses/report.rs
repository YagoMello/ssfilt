use std::fs;
use std::path::Path;

use anyhow::Result;
use csv::WriterBuilder;

use crate::Topology;
use crate::measure::{DEFAULT_SAMPLES_PER_CYCLE, InspectionPoint};
use crate::render::ResponseData;

const CSV_HEADERS: [&str; 24] = [
    "topology",
    "response",
    "final_order",
    "prototype_order",
    "lower_minus_three_db_hz",
    "upper_minus_three_db_hz",
    "normalized_frequency",
    "input_model",
    "samples_per_cycle",
    "measurement_status",
    "analytic_gain_db",
    "measured_inspection_gain",
    "measured_inspection_gain_db",
    "gain_error_db",
    "analytic_phase_radians",
    "measured_inspection_wrapped_phase_radians",
    "measured_inspection_phase_nearest_analytic_branch_radians",
    "phase_error_radians",
    "normalized_group_delay",
    "phase_resolved",
    "fit_offset",
    "fit_rms",
    "public_update_count",
    "fit_sample_count",
];

pub(super) fn write_csv(
    path: &Path,
    topology: Topology,
    final_order: usize,
    responses: &[ResponseData],
) -> Result<()> {
    fs::write(path, csv_bytes(topology, final_order, responses)?)?;
    Ok(())
}

fn csv_bytes(
    topology: Topology,
    final_order: usize,
    responses: &[ResponseData],
) -> Result<Vec<u8>> {
    let mut writer = WriterBuilder::new()
        .has_headers(false)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer.write_record(CSV_HEADERS)?;
    let prototype_order = topology.prototype_order(final_order).to_string();
    let final_order = final_order.to_string();
    for response in responses {
        for inspection in &response.measurements {
            write_row(
                &mut writer,
                topology,
                response.spec.label,
                &final_order,
                &prototype_order,
                *inspection,
            )?;
        }
    }
    writer.flush()?;
    writer
        .into_inner()
        .map_err(csv::IntoInnerError::into_error)
        .map_err(Into::into)
}

fn write_row<W: std::io::Write>(
    writer: &mut csv::Writer<W>,
    topology: Topology,
    response_label: &str,
    final_order: &str,
    prototype_order: &str,
    inspection: InspectionPoint,
) -> Result<()> {
    let (lower_edge_hz, upper_edge_hz) = topology.minus_three_db_edges();
    let analytic = inspection.analytic;
    let measured_fields = inspection.measured.map_or_else(
        || {
            [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]
        },
        |measured| {
            let gain_error = if analytic.gain_db.is_finite() {
                float(measured.gain_db - analytic.gain_db)
            } else {
                String::new()
            };
            let phase_error = measured
                .unwrapped_phase_radians
                .zip(analytic.phase_radians)
                .map(|(measured, analytic)| float(measured - analytic))
                .unwrap_or_default();
            [
                float(measured.gain),
                float(measured.gain_db),
                gain_error,
                float(measured.wrapped_phase_radians),
                float_option(measured.unwrapped_phase_radians),
                phase_error,
                (measured.phase_is_resolved() && analytic.phase_radians.is_some()).to_string(),
                float(measured.fit_offset),
                float(measured.fit_rms),
                measured.fit_sample_count.to_string(),
            ]
        },
    );
    let fields = [
        topology.label().to_owned(),
        response_label.to_owned(),
        final_order.to_owned(),
        prototype_order.to_owned(),
        float(lower_edge_hz),
        float(upper_edge_hz),
        float(inspection.ratio),
        "linear".to_owned(),
        DEFAULT_SAMPLES_PER_CYCLE.to_string(),
        inspection.status.label().to_owned(),
        float(analytic.gain_db),
        measured_fields[0].clone(),
        measured_fields[1].clone(),
        measured_fields[2].clone(),
        float_option(analytic.phase_radians),
        measured_fields[3].clone(),
        measured_fields[4].clone(),
        measured_fields[5].clone(),
        float_option(analytic.normalized_group_delay),
        measured_fields[6].clone(),
        measured_fields[7].clone(),
        measured_fields[8].clone(),
        inspection.public_update_count.to_string(),
        measured_fields[9].clone(),
    ];
    writer.write_record(fields)?;
    Ok(())
}

fn float(value: f64) -> String {
    format!("{value:.17}")
}

fn float_option(value: Option<f64>) -> String {
    value.map(float).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use plotters::prelude::RGBColor;
    use ssfilt::Response;

    use super::csv_bytes;
    use crate::Topology;
    use crate::analytic::FrequencyPoint;
    use crate::measure::{InspectionPoint, InspectionStatus, MeasuredFrequencyPoint};
    use crate::render::{ResponseData, ResponseSpec};

    #[test]
    fn csv_rows_are_stable_and_label_public_stream_inspection_points() {
        let responses = [ResponseData {
            spec: ResponseSpec::new("test, response", Response::Butterworth, RGBColor(0, 0, 0)),
            frequency: vec![FrequencyPoint {
                ratio: 1.0,
                gain_db: -3.010_299_956_639_812,
                phase_radians: Some(-1.0),
                normalized_group_delay: Some(0.5),
            }],
            measurements: vec![
                InspectionPoint {
                    ratio: 1.0,
                    analytic: FrequencyPoint {
                        ratio: 1.0,
                        gain_db: -3.010_299_956_639_812,
                        phase_radians: Some(-1.0),
                        normalized_group_delay: Some(0.5),
                    },
                    measured: Some(MeasuredFrequencyPoint {
                        gain: 0.707_106_781_186_547_6,
                        gain_db: -3.010_299_956_639_812,
                        fit_offset: 0.0,
                        wrapped_phase_radians: -1.0,
                        unwrapped_phase_radians: Some(-1.0),
                        fit_rms: 1.0e-8,
                        fit_sample_count: 1_024,
                    }),
                    public_update_count: 2_000,
                    status: InspectionStatus::Measured,
                },
                InspectionPoint {
                    ratio: 2.0,
                    analytic: FrequencyPoint {
                        ratio: 2.0,
                        gain_db: -12.0,
                        phase_radians: Some(-2.0),
                        normalized_group_delay: Some(0.25),
                    },
                    measured: None,
                    public_update_count: 2_000_000,
                    status: InspectionStatus::WorkLimitEstimate,
                },
            ],
            step: Vec::new(),
        }];

        let first = csv_bytes(Topology::LowPass, 2, &responses).unwrap();
        let second = csv_bytes(Topology::LowPass, 2, &responses).unwrap();
        assert_eq!(first, second);
        assert!(first.windows(2).all(|pair| pair != b"\r\n"));
        let mut reader = csv::Reader::from_reader(first.as_slice());
        let headers = reader.headers().unwrap().clone();
        assert_eq!(headers.len(), 24);
        assert_eq!(headers.get(7), Some("input_model"));
        assert_eq!(headers.get(8), Some("samples_per_cycle"));
        assert_eq!(headers.get(9), Some("measurement_status"));
        assert_eq!(headers.get(11), Some("measured_inspection_gain"));
        assert_eq!(headers.get(22), Some("public_update_count"));
        let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get(1), Some("test, response"));
        assert_eq!(rows[0].get(7), Some("linear"));
        assert_eq!(rows[0].get(8), Some("256"));
        assert_eq!(rows[0].get(9), Some("measured"));
        assert_eq!(rows[0].get(2), Some("2"));
        assert_eq!(rows[0].get(3), Some("2"));
        assert_eq!(rows[0].get(4), Some("1.00000000000000000"));
        assert_eq!(rows[0].get(5), Some("1.00000000000000000"));
        assert_eq!(rows[0].get(22), Some("2000"));
        assert_eq!(rows[1].get(9), Some("work_limit_estimate"));
        assert_eq!(rows[1].get(10), Some("-12.00000000000000000"));
        assert_eq!(rows[1].get(11), Some(""));
        assert_eq!(rows[1].get(22), Some("2000000"));

        let band_pass = csv_bytes(Topology::BandPass, 4, &responses).unwrap();
        let mut reader = csv::Reader::from_reader(band_pass.as_slice());
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.get(2), Some("4"));
        assert_eq!(row.get(3), Some("2"));
        assert_eq!(row.get(4), Some("0.50000000000000000"));
        assert_eq!(row.get(5), Some("2.00000000000000000"));
    }

    #[test]
    fn band_stop_center_keeps_gain_evidence_without_inventing_phase() {
        let responses = [ResponseData {
            spec: ResponseSpec::new("Butterworth", Response::Butterworth, RGBColor(0, 0, 0)),
            frequency: Vec::new(),
            measurements: vec![InspectionPoint {
                ratio: 1.0,
                analytic: FrequencyPoint {
                    ratio: 1.0,
                    gain_db: f64::NEG_INFINITY,
                    phase_radians: None,
                    normalized_group_delay: None,
                },
                measured: Some(MeasuredFrequencyPoint {
                    gain: 1.0e-5,
                    gain_db: -100.0,
                    fit_offset: 0.0,
                    wrapped_phase_radians: 0.75,
                    unwrapped_phase_radians: None,
                    fit_rms: 1.0e-8,
                    fit_sample_count: 1_024,
                }),
                public_update_count: 2_000,
                status: InspectionStatus::Measured,
            }],
            step: Vec::new(),
        }];
        let bytes = csv_bytes(Topology::BandStop, 4, &responses).unwrap();
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.get(10), Some("-inf"));
        assert_eq!(row.get(11), Some("0.00001000000000000"));
        assert_eq!(row.get(13), Some(""));
        assert_eq!(row.get(14), Some(""));
        assert_eq!(row.get(16), Some(""));
        assert_eq!(row.get(17), Some(""));
        assert_eq!(row.get(18), Some(""));
        assert_eq!(row.get(19), Some("false"));
    }
}
