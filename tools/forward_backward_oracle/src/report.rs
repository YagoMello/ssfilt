//! Deterministic artifacts for the filtered forward-backward diagnostic.

use core::fmt;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

use crate::{candidate::CandidateWorkCounters, provenance::SourceProvenance};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(crate) struct CellReportRow {
    pub case_id: u64,
    pub record_digest: String,
    pub cutoff: &'static str,
    pub variant: &'static str,
    pub interval_count: usize,
    pub sample_count: usize,
    pub status: &'static str,
    pub oracle_rms: String,
    pub oracle_peak: String,
    pub oracle_low_imaginary: String,
    pub oracle_high_imaginary: String,
    pub collinear_peak: Option<String>,
    pub collinear_limit: Option<String>,
    pub collinear_gate: &'static str,
    pub candidate_rms: Option<String>,
    pub candidate_rms_limit: Option<String>,
    pub candidate_rms_gate: &'static str,
    pub candidate_peak: Option<String>,
    pub candidate_peak_limit: Option<String>,
    pub candidate_peak_gate: &'static str,
    pub tail_mode: &'static str,
    pub tail_gate: &'static str,
    pub work: Option<CandidateWorkCounters>,
    pub total_accepted_steps: Option<u64>,
    pub total_rejected_steps: Option<u64>,
    pub total_derivative_evaluations: Option<u64>,
    pub aggregate_work_gate: &'static str,
    pub per_interval_work_gate: &'static str,
    pub workspace_gate: &'static str,
    pub edge_loss_gate: &'static str,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ComparisonReportRow {
    pub comparison: &'static str,
    pub first_case_id: u64,
    pub second_case_id: u64,
    pub status: &'static str,
    pub normalized_rms: Option<String>,
    pub rms_limit: Option<String>,
    pub rms_gate: &'static str,
    pub normalized_peak: Option<String>,
    pub peak_limit: Option<String>,
    pub peak_gate: &'static str,
    pub detail: String,
}

pub(crate) struct ReportBundle<'a> {
    pub manifest: &'a [u8],
    pub coordinate_digest_hex: String,
    pub provenance: &'a SourceProvenance,
    pub cells: &'a [CellReportRow],
    pub comparisons: &'a [ComparisonReportRow],
    pub observational_elapsed_ns: u128,
}

#[derive(Debug)]
pub(crate) enum ReportError {
    OutputExists(PathBuf),
    MissingParent(PathBuf),
    InvalidOutputName(PathBuf),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ReportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputExists(path) => write!(
                formatter,
                "report output already exists and will not be overwritten: {}",
                path.display()
            ),
            Self::MissingParent(path) => {
                write!(formatter, "report output has no parent: {}", path.display())
            }
            Self::InvalidOutputName(path) => write!(
                formatter,
                "report output has no usable final component: {}",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "could not {operation} report path {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn publish_report(output: &Path, bundle: &ReportBundle<'_>) -> Result<(), ReportError> {
    if output.exists() {
        return Err(ReportError::OutputExists(output.to_path_buf()));
    }
    let parent = report_parent(output)?;
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ReportError::InvalidOutputName(output.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| ReportError::Io {
        operation: "create parent directory for",
        path: parent.to_path_buf(),
        source,
    })?;

    let nonce = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.tmp.{}.{}", std::process::id(), nonce));
    fs::create_dir(&temporary).map_err(|source| ReportError::Io {
        operation: "create temporary directory for",
        path: temporary.clone(),
        source,
    })?;

    let result = write_report_files(&temporary, bundle).and_then(|()| {
        fs::rename(&temporary, output).map_err(|source| ReportError::Io {
            operation: "publish",
            path: output.to_path_buf(),
            source,
        })
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn report_parent(output: &Path) -> Result<&Path, ReportError> {
    let parent = output
        .parent()
        .ok_or_else(|| ReportError::MissingParent(output.to_path_buf()))?;
    Ok(if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    })
}

fn write_report_files(directory: &Path, bundle: &ReportBundle<'_>) -> Result<(), ReportError> {
    let files = [
        ("manifest.csv", bundle.manifest.to_vec()),
        ("cells.csv", cells_csv(bundle.cells)),
        ("comparisons.csv", comparisons_csv(bundle.comparisons)),
        ("source-files.csv", source_files_csv(bundle.provenance)),
        ("summary.md", summary(bundle).into_bytes()),
    ];
    for (name, contents) in &files {
        write_file(directory, name, contents)?;
    }
    let checksums = checksums_file(&files);
    write_file(directory, "checksums.sha256", &checksums)
}

fn write_file(directory: &Path, name: &str, contents: &[u8]) -> Result<(), ReportError> {
    let path = directory.join(name);
    fs::write(&path, contents).map_err(|source| ReportError::Io {
        operation: "write",
        path,
        source,
    })
}

fn cells_csv(rows: &[CellReportRow]) -> Vec<u8> {
    let mut output = String::from(
        "case_id,record_digest,cutoff,variant,interval_count,sample_count,status,oracle_normalized_rms,oracle_normalized_peak,oracle_low_imaginary_ratio,oracle_high_imaginary_ratio,collinear_normalized_peak,collinear_limit,collinear_gate,candidate_normalized_rms,candidate_rms_limit,candidate_rms_gate,candidate_normalized_peak,candidate_peak_limit,candidate_peak_gate,tail_mode,tail_gate,forward_accepted_steps,forward_rejected_steps,forward_derivative_evaluations,tail_accepted_steps,tail_rejected_steps,tail_derivative_evaluations,reverse_accepted_steps,reverse_rejected_steps,reverse_derivative_evaluations,total_accepted_steps,total_rejected_steps,total_derivative_evaluations,aggregate_work_gate,per_interval_work_gate,workspace_gate,edge_loss_gate,detail\n",
    );
    for row in rows {
        let values = vec![
            format!("0x{:016x}", row.case_id),
            row.record_digest.clone(),
            row.cutoff.to_owned(),
            row.variant.to_owned(),
            row.interval_count.to_string(),
            row.sample_count.to_string(),
            row.status.to_owned(),
            row.oracle_rms.clone(),
            row.oracle_peak.clone(),
            row.oracle_low_imaginary.clone(),
            row.oracle_high_imaginary.clone(),
            option_text(&row.collinear_peak),
            option_text(&row.collinear_limit),
            row.collinear_gate.to_owned(),
            option_text(&row.candidate_rms),
            option_text(&row.candidate_rms_limit),
            row.candidate_rms_gate.to_owned(),
            option_text(&row.candidate_peak),
            option_text(&row.candidate_peak_limit),
            row.candidate_peak_gate.to_owned(),
            row.tail_mode.to_owned(),
            row.tail_gate.to_owned(),
            pass_counter(row.work, CandidateWorkCounters::forward, |pass| {
                pass.accepted_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::forward, |pass| {
                pass.rejected_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::forward, |pass| {
                pass.derivative_evaluations
            }),
            pass_counter(row.work, CandidateWorkCounters::tail, |pass| {
                pass.accepted_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::tail, |pass| {
                pass.rejected_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::tail, |pass| {
                pass.derivative_evaluations
            }),
            pass_counter(row.work, CandidateWorkCounters::reverse, |pass| {
                pass.accepted_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::reverse, |pass| {
                pass.rejected_steps
            }),
            pass_counter(row.work, CandidateWorkCounters::reverse, |pass| {
                pass.derivative_evaluations
            }),
            option_number(row.total_accepted_steps),
            option_number(row.total_rejected_steps),
            option_number(row.total_derivative_evaluations),
            row.aggregate_work_gate.to_owned(),
            row.per_interval_work_gate.to_owned(),
            row.workspace_gate.to_owned(),
            row.edge_loss_gate.to_owned(),
            row.detail.clone(),
        ];
        push_csv_row(&mut output, &values);
    }
    output.into_bytes()
}

fn comparisons_csv(rows: &[ComparisonReportRow]) -> Vec<u8> {
    let mut output = String::from(
        "comparison,first_case_id,second_case_id,status,normalized_rms,rms_limit,rms_gate,normalized_peak,peak_limit,peak_gate,detail\n",
    );
    for row in rows {
        let values = vec![
            row.comparison.to_owned(),
            format!("0x{:016x}", row.first_case_id),
            format!("0x{:016x}", row.second_case_id),
            row.status.to_owned(),
            option_text(&row.normalized_rms),
            option_text(&row.rms_limit),
            row.rms_gate.to_owned(),
            option_text(&row.normalized_peak),
            option_text(&row.peak_limit),
            row.peak_gate.to_owned(),
            row.detail.clone(),
        ];
        push_csv_row(&mut output, &values);
    }
    output.into_bytes()
}

fn source_files_csv(provenance: &SourceProvenance) -> Vec<u8> {
    let mut output = String::from("relative_path,kind,byte_length,sha256\n");
    for entry in provenance.entries() {
        push_csv_row(
            &mut output,
            &[
                entry.relative_path().to_owned(),
                entry.kind().as_str().to_owned(),
                entry.byte_length().to_string(),
                entry.sha256_hex(),
            ],
        );
    }
    output.into_bytes()
}

fn summary(bundle: &ReportBundle<'_>) -> String {
    let passed = bundle
        .cells
        .iter()
        .filter(|row| row.status == "passed")
        .count();
    let failed = bundle
        .cells
        .iter()
        .filter(|row| row.status == "failed")
        .count();
    let errored = bundle
        .cells
        .iter()
        .filter(|row| row.status == "errored")
        .count();
    format!(
        "# Forward-backward filtered diagnostic\n\n\
         This report is an integrity-complete **filtered diagnostic**. It is not a complete matrix run and is not acceptance evidence.\n\n\
         | Field | Value |\n| --- | --- |\n\
         | Schema version | 1 |\n\
         | Tool version | {} |\n\
         | Protocol version | {} |\n\
         | Complete | false |\n\
         | Acceptance eligible | false |\n\
         | Candidate | endpoint-replay negative control |\n\
         | Selected cells | {} |\n\
         | Passed cells | {passed} |\n\
         | Failed cells | {failed} |\n\
         | Errored cells | {errored} |\n\
         | Comparison rows | {} |\n\
         | Coordinate digest | `{}` |\n\
         | Source digest | `{}` |\n\
         | Observational elapsed time | {} ns |\n\n\
         A missing physical forward tail is a local candidate failure even when whole-record RMS and peak happen to pass. Protocol v1 exposes work for each pass but not per-interval or workspace measurements, so those gates are recorded as `not_evaluated`. Edge-loss diagnostics are also `not_evaluated` in this first slice. Reversal rows are measurements only; they deliberately have no invented acceptance threshold. Elapsed time is a non-gating observation measured before artifact publication.\n",
        env!("CARGO_PKG_VERSION"),
        forward_backward_protocol::PROTOCOL_VERSION,
        bundle.cells.len(),
        bundle.comparisons.len(),
        bundle.coordinate_digest_hex,
        bundle.provenance.source_digest_hex(),
        bundle.observational_elapsed_ns,
    )
}

fn checksums_file(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut output = String::new();
    for (name, contents) in files {
        output.push_str(&hex(&Sha256::digest(contents)));
        output.push_str("  ");
        output.push_str(name);
        output.push('\n');
    }
    output.into_bytes()
}

fn option_text(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

fn option_number(value: Option<u64>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

fn pass_counter(
    work: Option<CandidateWorkCounters>,
    pass: impl FnOnce(CandidateWorkCounters) -> forward_backward_protocol::PassCounters,
    field: impl FnOnce(forward_backward_protocol::PassCounters) -> u64,
) -> String {
    work.map_or_else(String::new, |work| field(pass(work)).to_string())
}

fn push_csv_row(output: &mut String, values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        push_csv_field(output, value);
    }
    output.push('\n');
}

fn push_csv_field(output: &mut String, value: &str) {
    if value
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\n' | b'\r'))
    {
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

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::collect_source_provenance;

    #[test]
    fn csv_fields_quote_commas_quotes_and_newlines() {
        let mut output = String::new();
        push_csv_row(&mut output, &["plain".into(), "a,b\"c\nd".into()]);
        assert_eq!(output, "plain,\"a,b\"\"c\nd\"\n");
    }

    #[test]
    fn hexadecimal_encoding_is_lowercase_and_fixed_width() {
        assert_eq!(hex(&[0, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    #[test]
    fn cell_schema_keeps_per_pass_work_and_every_deferred_gate_explicit() {
        let rendered = String::from_utf8(cells_csv(&[])).unwrap();
        for column in [
            "forward_accepted_steps",
            "forward_rejected_steps",
            "forward_derivative_evaluations",
            "tail_accepted_steps",
            "tail_rejected_steps",
            "tail_derivative_evaluations",
            "reverse_accepted_steps",
            "reverse_rejected_steps",
            "reverse_derivative_evaluations",
            "aggregate_work_gate",
            "per_interval_work_gate",
            "workspace_gate",
            "edge_loss_gate",
        ] {
            assert!(rendered.split(',').any(|field| field.trim() == column));
        }
    }

    #[test]
    fn report_publication_is_atomic_and_refuses_overwrite() {
        let manifest = b"schema_version\n1\n";
        let provenance = collect_source_provenance(manifest).unwrap();
        let bundle = ReportBundle {
            manifest,
            coordinate_digest_hex: hex(&Sha256::digest(manifest)),
            provenance: &provenance,
            cells: &[],
            comparisons: &[],
            observational_elapsed_ns: 123,
        };
        let output = std::env::temp_dir().join(format!(
            "ssfilt-report-test-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!output.exists());

        publish_report(&output, &bundle).unwrap();
        for name in [
            "manifest.csv",
            "cells.csv",
            "comparisons.csv",
            "source-files.csv",
            "summary.md",
            "checksums.sha256",
        ] {
            assert!(output.join(name).is_file(), "missing {name}");
        }
        assert!(matches!(
            publish_report(&output, &bundle),
            Err(ReportError::OutputExists(path)) if path == output
        ));

        fs::remove_dir_all(&output).unwrap();
    }

    #[test]
    fn single_component_output_uses_the_current_directory() {
        assert_eq!(
            report_parent(Path::new("report-name")).unwrap(),
            Path::new(".")
        );
    }

    #[test]
    fn summary_records_required_versions_and_elapsed_observation() {
        let manifest = b"schema_version\n1\n";
        let provenance = collect_source_provenance(manifest).unwrap();
        let bundle = ReportBundle {
            manifest,
            coordinate_digest_hex: hex(&Sha256::digest(manifest)),
            provenance: &provenance,
            cells: &[],
            comparisons: &[],
            observational_elapsed_ns: 123,
        };
        let rendered = summary(&bundle);
        assert!(rendered.contains("| Tool version | 0.1.0 |"));
        assert!(rendered.contains("| Protocol version | 1 |"));
        assert!(rendered.contains("| Observational elapsed time | 123 ns |"));
    }
}
