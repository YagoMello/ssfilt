//! First report-producing forward-backward diagnostic slice.

use std::{path::Path, time::Instant};

use forward_backward_protocol::{PassCounters, TailEvidence};
use rug::Float;

use crate::{
    HIGH_ORACLE_PRECISION, PhysicalCutoff, RepresentedRecord, ScalarRefinement,
    build_nominal_record, build_represented_case,
    candidate::CandidateWorkCounters,
    comparison::{
        CrossCellOutcome, compare_partition_candidates, compare_physical_scaling_candidates,
    },
    identity::{CellVariant, ResponseCellKey},
    manifest::{ManifestEntry, cell_variant_tag, filtered_sentinel_manifest, physical_cutoff_tag},
    matrix::{ResponseCellAssessment, execute_response_cell},
    oracle::{CollinearRefinementAssessment, assess_exact_collinear_refinement},
    provenance::collect_source_provenance,
    refine_represented_case,
    report::{CellReportRow, ComparisonReportRow, ReportBundle, hex, publish_report},
    reverse_record,
    score::CandidateDisposition,
    session::CandidateSession,
    weighted_error,
};

struct PreparedCell {
    entry: ManifestEntry,
    record: RepresentedRecord,
    assessment: ResponseCellAssessment,
}

struct PreparedCutoff {
    cutoff: PhysicalCutoff,
    base: RepresentedRecord,
    refinement: ScalarRefinement,
    reversed_base: RepresentedRecord,
    reversed_refined: RepresentedRecord,
    collinear: CollinearRefinementAssessment,
}

/// Run the fixed eight-cell reverse-start-sentinel diagnostic and atomically
/// publish its compact evidence. Numerical candidate failures remain report
/// rows; provenance, transport, and oracle-integrity failures abort the run.
pub fn run_filtered_matrix(output: &Path) -> Result<String, String> {
    let started = Instant::now();
    let manifest = filtered_sentinel_manifest().map_err(|error| error.to_string())?;
    let selection = manifest.selection();
    let nominal = build_nominal_record(selection.fixture_coordinate())
        .map_err(|error| format!("could not build selected nominal fixture: {error}"))?;

    let mut prepared = Vec::with_capacity(2);
    for &cutoff in selection.cutoffs() {
        let represented = build_represented_case(&nominal, selection.scalar(), cutoff)
            .map_err(|error| format!("could not represent {cutoff:?} fixture: {error}"))?;
        let refinement = refine_represented_case(&represented)
            .map_err(|error| format!("could not refine {cutoff:?} fixture: {error}"))?;
        let reversed_base = reverse_record(represented.record())
            .map_err(|error| format!("could not reverse {cutoff:?} base: {error}"))?;
        let reversed_refined = reverse_record(refinement.record())
            .map_err(|error| format!("could not reverse {cutoff:?} refinement: {error}"))?;
        let collinear = assess_exact_collinear_refinement(8, represented.record());
        if !collinear.passed() {
            return Err(format!(
                "exact-collinear oracle invariant failed for {cutoff:?}: {} exceeds {}",
                collinear.normalized_peak(),
                collinear.limit()
            ));
        }
        prepared.push(PreparedCutoff {
            cutoff,
            base: represented.record().clone(),
            refinement,
            reversed_base: reversed_base.record().clone(),
            reversed_refined: reversed_refined.record().clone(),
            collinear,
        });
    }

    let mut session = CandidateSession::start().map_err(|error| error.to_string())?;
    let mut cells = Vec::with_capacity(manifest.entries().len());
    for &entry in manifest.entries() {
        let record = selected_record(&prepared, entry.key())?.clone();
        let assessment = execute_response_cell(&mut session, &entry.key(), &record)
            .map_err(|error| error.to_string())?;
        cells.push(PreparedCell {
            entry,
            record,
            assessment,
        });
    }
    session.finish().map_err(|error| error.to_string())?;

    let cell_rows = cells
        .iter()
        .map(|cell| cell_report_row(cell, &prepared))
        .collect::<Result<Vec<_>, _>>()?;
    let comparison_rows = comparison_rows(&cells, &prepared)?;
    let manifest_bytes = manifest.canonical_bytes();
    let provenance =
        collect_source_provenance(&manifest_bytes).map_err(|error| error.to_string())?;
    let observational_elapsed_ns = started.elapsed().as_nanos();
    let bundle = ReportBundle {
        manifest: &manifest_bytes,
        coordinate_digest_hex: hex(&manifest.coordinate_digest()),
        provenance: &provenance,
        cells: &cell_rows,
        comparisons: &comparison_rows,
        observational_elapsed_ns,
    };
    publish_report(output, &bundle).map_err(|error| error.to_string())?;

    let failed = cell_rows
        .iter()
        .filter(|row| row.status == "failed")
        .count();
    let errored = cell_rows
        .iter()
        .filter(|row| row.status == "errored")
        .count();
    Ok(format!(
        "wrote filtered diagnostic with {} cells ({failed} failed, {errored} errored) and {} comparisons to {}",
        cell_rows.len(),
        comparison_rows.len(),
        output.display()
    ))
}

fn selected_record(
    prepared: &[PreparedCutoff],
    key: ResponseCellKey,
) -> Result<&RepresentedRecord, String> {
    let cutoff = prepared
        .iter()
        .find(|prepared| prepared.cutoff == key.cutoff())
        .ok_or_else(|| format!("no prepared record for {:?}", key.cutoff()))?;
    Ok(match key.variant() {
        CellVariant::Base => &cutoff.base,
        CellVariant::ScalarRefined => cutoff.refinement.record(),
        CellVariant::ReversedBase => &cutoff.reversed_base,
        CellVariant::ReversedScalarRefined => &cutoff.reversed_refined,
    })
}

fn cell_report_row(
    cell: &PreparedCell,
    prepared: &[PreparedCutoff],
) -> Result<CellReportRow, String> {
    let identity = cell.assessment.identity();
    let oracle = cell.assessment.assessment().oracle();
    let convergence = oracle.convergence();
    let key = cell.entry.key();
    let collinear = prepared
        .iter()
        .find(|prepared| prepared.cutoff == key.cutoff())
        .ok_or_else(|| "prepared collinear assessment disappeared".to_owned())?;
    let is_base = key.variant() == CellVariant::Base;

    let common = CellReportRow {
        case_id: identity.case_id,
        record_digest: hex(&identity.record_digest),
        cutoff: physical_cutoff_tag(key.cutoff()),
        variant: cell_variant_tag(key.variant()),
        interval_count: cell.record.intervals_seconds().len(),
        sample_count: cell.record.samples().len(),
        status: "errored",
        oracle_rms: decimal(convergence.normalized_rms()),
        oracle_peak: decimal(convergence.normalized_peak()),
        oracle_low_imaginary: decimal(oracle.low_imaginary_ratio()),
        oracle_high_imaginary: decimal(oracle.high_imaginary_ratio()),
        collinear_peak: is_base.then(|| decimal(collinear.collinear.normalized_peak())),
        collinear_limit: is_base.then(|| decimal(collinear.collinear.limit())),
        collinear_gate: if is_base { "pass" } else { "not_applicable" },
        candidate_rms: None,
        candidate_rms_limit: None,
        candidate_rms_gate: "not_evaluated",
        candidate_peak: None,
        candidate_peak_limit: None,
        candidate_peak_gate: "not_evaluated",
        tail_mode: "not_available",
        tail_gate: "not_evaluated",
        work: None,
        total_accepted_steps: None,
        total_rejected_steps: None,
        total_derivative_evaluations: None,
        aggregate_work_gate: "not_evaluated",
        per_interval_work_gate: "not_evaluated",
        workspace_gate: "not_evaluated",
        edge_loss_gate: "not_evaluated",
        detail: String::new(),
    };

    Ok(match cell.assessment.assessment().candidate() {
        CandidateDisposition::Scored {
            metrics,
            gates,
            work,
            tail_evidence,
            ..
        } => {
            let (accepted, rejected, derivatives) = total_work(*work)?;
            let work_passed = aggregate_work_passes(
                cell.record.intervals_seconds().len(),
                accepted,
                rejected,
                derivatives,
            )?;
            let (tail_mode, tail_gate, tail_passed) = tail_classification(tail_evidence);
            CellReportRow {
                status: if gates.passed() && tail_passed && work_passed {
                    "passed"
                } else {
                    "failed"
                },
                candidate_rms: Some(decimal(metrics.normalized_rms())),
                candidate_rms_limit: Some(decimal(gates.rms().limit())),
                candidate_rms_gate: gate(gates.rms().passed()),
                candidate_peak: Some(decimal(metrics.normalized_peak())),
                candidate_peak_limit: Some(decimal(gates.peak().limit())),
                candidate_peak_gate: gate(gates.peak().passed()),
                tail_mode,
                tail_gate,
                work: Some(*work),
                total_accepted_steps: Some(accepted),
                total_rejected_steps: Some(rejected),
                total_derivative_evaluations: Some(derivatives),
                aggregate_work_gate: gate(work_passed),
                detail: if tail_passed {
                    String::new()
                } else {
                    "candidate omitted the physical forward constant-continuation tail".to_owned()
                },
                ..common
            }
        }
        CandidateDisposition::CandidateError(error) => CellReportRow {
            detail: format!(
                "candidate_error category={} pass={} interval={} source={}",
                candidate_category_tag(error.category),
                error.pass.map_or("none", pass_tag),
                error
                    .interval
                    .map_or_else(|| "none".to_owned(), |value| value.to_string()),
                error.source_category.map_or("none", source_category_tag),
            ),
            ..common
        },
        CandidateDisposition::NonFiniteSuccess(error) => {
            let work = error.work();
            let (accepted, rejected, derivatives) = total_work(work)?;
            let (tail_mode, tail_gate, _) = tail_classification(error.tail_evidence());
            CellReportRow {
                tail_mode,
                tail_gate,
                work: Some(work),
                total_accepted_steps: Some(accepted),
                total_rejected_steps: Some(rejected),
                total_derivative_evaluations: Some(derivatives),
                detail: format!("non_finite_success output_index={}", error.index()),
                ..common
            }
        }
    })
}

fn comparison_rows(
    cells: &[PreparedCell],
    prepared: &[PreparedCutoff],
) -> Result<Vec<ComparisonReportRow>, String> {
    let mut rows = Vec::with_capacity(12);

    for &cutoff in &[PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz] {
        let base = find_cell(cells, cutoff, CellVariant::Base)?;
        let refined = find_cell(cells, cutoff, CellVariant::ScalarRefined)?;
        let records = prepared_for(prepared, cutoff)?;
        let outcome = compare_partition_candidates(
            &base.assessment,
            &base.record,
            &records.refinement,
            &refined.assessment,
        )
        .map_err(|error| error.to_string())?;
        rows.push(partition_row(base, refined, outcome));

        let reversed_base = find_cell(cells, cutoff, CellVariant::ReversedBase)?;
        let reversed_refined = find_cell(cells, cutoff, CellVariant::ReversedScalarRefined)?;
        rows.push(direct_partition_row(
            reversed_base,
            reversed_refined,
            &records.refinement,
        )?);
    }

    for &variant in &[
        CellVariant::Base,
        CellVariant::ScalarRefined,
        CellVariant::ReversedBase,
        CellVariant::ReversedScalarRefined,
    ] {
        let one = find_cell(cells, PhysicalCutoff::OneHz, variant)?;
        let thirty_seven = find_cell(cells, PhysicalCutoff::ThirtySevenHz, variant)?;
        let outcome = compare_physical_scaling_candidates(
            &one.assessment,
            &one.record,
            &thirty_seven.assessment,
            &thirty_seven.record,
        )
        .map_err(|error| error.to_string())?;
        rows.push(physical_scaling_row(one, thirty_seven, outcome));
    }

    for &cutoff in &[PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz] {
        for &(forward_variant, reversed_variant) in &[
            (CellVariant::Base, CellVariant::ReversedBase),
            (
                CellVariant::ScalarRefined,
                CellVariant::ReversedScalarRefined,
            ),
        ] {
            let forward = find_cell(cells, cutoff, forward_variant)?;
            let reversed = find_cell(cells, cutoff, reversed_variant)?;
            rows.push(reversal_row(forward, reversed)?);
        }
    }
    Ok(rows)
}

fn partition_row(
    first: &PreparedCell,
    second: &PreparedCell,
    outcome: CrossCellOutcome<crate::comparison::PartitionComparisonMetrics>,
) -> ComparisonReportRow {
    match outcome {
        CrossCellOutcome::Eligible(metrics) => ComparisonReportRow {
            comparison: "partition_invariance",
            first_case_id: first.entry.case_id(),
            second_case_id: second.entry.case_id(),
            status: if metrics.normalized_rms().passed() && metrics.normalized_peak().passed() {
                "passed"
            } else {
                "failed"
            },
            normalized_rms: Some(decimal(metrics.normalized_rms().measured())),
            rms_limit: Some(decimal(metrics.normalized_rms().limit())),
            rms_gate: gate(metrics.normalized_rms().passed()),
            normalized_peak: Some(decimal(metrics.normalized_peak().measured())),
            peak_limit: Some(decimal(metrics.normalized_peak().limit())),
            peak_gate: gate(metrics.normalized_peak().passed()),
            detail: String::new(),
        },
        CrossCellOutcome::Ineligible(_) => ineligible_comparison(
            "partition_invariance",
            first,
            second,
            "one or both whole-record candidate outcomes were ineligible",
        ),
    }
}

fn direct_partition_row(
    first: &PreparedCell,
    second: &PreparedCell,
    refinement: &ScalarRefinement,
) -> Result<ComparisonReportRow, String> {
    let (Some(first_output), Some(second_output)) =
        (eligible_output(first), eligible_output(second))
    else {
        return Ok(ineligible_comparison(
            "partition_invariance",
            first,
            second,
            "one or both reversed whole-record candidate outcomes were ineligible",
        ));
    };
    let aligned = crate::extract_original_knots(refinement, second_output)
        .map_err(|error| error.to_string())?;
    let metrics = weighted_error(&first.record, first_output, &aligned, HIGH_ORACLE_PRECISION)
        .map_err(|error| error.to_string())?;
    let rms_limit = rational(25, 1_000_000);
    let peak_limit = rational(1, 10_000);
    let rms_passed = metrics.normalized_rms() <= &rms_limit;
    let peak_passed = metrics.normalized_peak() <= &peak_limit;
    Ok(ComparisonReportRow {
        comparison: "partition_invariance",
        first_case_id: first.entry.case_id(),
        second_case_id: second.entry.case_id(),
        status: if rms_passed && peak_passed {
            "passed"
        } else {
            "failed"
        },
        normalized_rms: Some(decimal(metrics.normalized_rms())),
        rms_limit: Some(decimal(&rms_limit)),
        rms_gate: gate(rms_passed),
        normalized_peak: Some(decimal(metrics.normalized_peak())),
        peak_limit: Some(decimal(&peak_limit)),
        peak_gate: gate(peak_passed),
        detail: String::new(),
    })
}

fn physical_scaling_row(
    first: &PreparedCell,
    second: &PreparedCell,
    outcome: CrossCellOutcome<crate::comparison::PhysicalScalingComparisonMetrics>,
) -> ComparisonReportRow {
    match outcome {
        CrossCellOutcome::Eligible(metrics) => ComparisonReportRow {
            comparison: "physical_scaling",
            first_case_id: first.entry.case_id(),
            second_case_id: second.entry.case_id(),
            status: if metrics.normalized_peak().passed() {
                "passed"
            } else {
                "failed"
            },
            normalized_rms: None,
            rms_limit: None,
            rms_gate: "not_applicable",
            normalized_peak: Some(decimal(metrics.normalized_peak().measured())),
            peak_limit: Some(decimal(metrics.normalized_peak().limit())),
            peak_gate: gate(metrics.normalized_peak().passed()),
            detail: String::new(),
        },
        CrossCellOutcome::Ineligible(_) => ineligible_comparison(
            "physical_scaling",
            first,
            second,
            "one or both whole-record candidate outcomes were ineligible",
        ),
    }
}

fn reversal_row(
    forward: &PreparedCell,
    reversed: &PreparedCell,
) -> Result<ComparisonReportRow, String> {
    let (Some(forward_output), Some(reversed_output)) =
        (finite_output(forward), finite_output(reversed))
    else {
        return Ok(ineligible_comparison(
            "reversal_covariance",
            forward,
            reversed,
            "one or both whole-record candidate outcomes were ineligible",
        ));
    };
    let aligned = reversed_output.iter().rev().cloned().collect::<Vec<_>>();
    let metrics = weighted_error(
        &forward.record,
        forward_output,
        &aligned,
        HIGH_ORACLE_PRECISION,
    )
    .map_err(|error| error.to_string())?;
    Ok(ComparisonReportRow {
        comparison: "reversal_covariance",
        first_case_id: forward.entry.case_id(),
        second_case_id: reversed.entry.case_id(),
        status: "measured",
        normalized_rms: Some(decimal(metrics.normalized_rms())),
        rms_limit: None,
        rms_gate: "not_evaluated",
        normalized_peak: Some(decimal(metrics.normalized_peak())),
        peak_limit: None,
        peak_gate: "not_evaluated",
        detail: "diagnostic measurement; no reversal-comparison threshold is frozen".to_owned(),
    })
}

fn ineligible_comparison(
    kind: &'static str,
    first: &PreparedCell,
    second: &PreparedCell,
    detail: &str,
) -> ComparisonReportRow {
    ComparisonReportRow {
        comparison: kind,
        first_case_id: first.entry.case_id(),
        second_case_id: second.entry.case_id(),
        status: "ineligible",
        normalized_rms: None,
        rms_limit: None,
        rms_gate: "not_evaluated",
        normalized_peak: None,
        peak_limit: None,
        peak_gate: "not_evaluated",
        detail: detail.to_owned(),
    }
}

fn eligible_output(cell: &PreparedCell) -> Option<&[Float]> {
    match cell.assessment.assessment().candidate() {
        CandidateDisposition::Scored { output, gates, .. } if gates.passed() => Some(output),
        _ => None,
    }
}

fn finite_output(cell: &PreparedCell) -> Option<&[Float]> {
    cell.assessment.scored_output()
}

fn find_cell(
    cells: &[PreparedCell],
    cutoff: PhysicalCutoff,
    variant: CellVariant,
) -> Result<&PreparedCell, String> {
    cells
        .iter()
        .find(|cell| cell.entry.key().cutoff() == cutoff && cell.entry.key().variant() == variant)
        .ok_or_else(|| format!("missing {cutoff:?}/{variant:?} cell"))
}

fn prepared_for(
    prepared: &[PreparedCutoff],
    cutoff: PhysicalCutoff,
) -> Result<&PreparedCutoff, String> {
    prepared
        .iter()
        .find(|prepared| prepared.cutoff == cutoff)
        .ok_or_else(|| format!("missing prepared {cutoff:?} records"))
}

fn total_work(work: CandidateWorkCounters) -> Result<(u64, u64, u64), String> {
    let passes = [work.forward(), work.tail(), work.reverse()];
    Ok((
        checked_sum(&passes, |pass| pass.accepted_steps)?,
        checked_sum(&passes, |pass| pass.rejected_steps)?,
        checked_sum(&passes, |pass| pass.derivative_evaluations)?,
    ))
}

fn checked_sum(
    passes: &[PassCounters],
    field: impl Fn(PassCounters) -> u64,
) -> Result<u64, String> {
    passes.iter().copied().try_fold(0_u64, |total, pass| {
        total
            .checked_add(field(pass))
            .ok_or_else(|| "candidate work counter sum overflowed".to_owned())
    })
}

fn aggregate_work_passes(
    interval_count: usize,
    accepted: u64,
    rejected: u64,
    derivatives: u64,
) -> Result<bool, String> {
    let intervals = u64::try_from(interval_count)
        .map_err(|_| "record interval count does not fit u64".to_owned())?;
    let attempts = accepted
        .checked_add(rejected)
        .ok_or_else(|| "candidate attempt count overflowed".to_owned())?;
    let attempt_limit = 128_u64
        .checked_mul(intervals)
        .and_then(|value| value.checked_add(512))
        .ok_or_else(|| "endpoint attempt ceiling overflowed".to_owned())?;
    let derivative_limit = 896_u64
        .checked_mul(intervals)
        .and_then(|value| value.checked_add(3_584))
        .ok_or_else(|| "endpoint derivative ceiling overflowed".to_owned())?;
    Ok(attempts <= attempt_limit && derivatives <= derivative_limit)
}

fn tail_classification(evidence: &TailEvidence) -> (&'static str, &'static str, bool) {
    match evidence {
        TailEvidence::Omitted => ("omitted", "fail", false),
        TailEvidence::Analytic => ("analytic", "pass", true),
        TailEvidence::NumericalF32 { .. } => ("numerical_f32", "not_evaluated", false),
        TailEvidence::NumericalF64 { .. } => ("numerical_f64", "not_evaluated", false),
    }
}

fn decimal(value: &Float) -> String {
    value.to_string_radix(10, Some(80))
}

fn rational(numerator: u32, denominator: u32) -> Float {
    let mut value = Float::with_val(HIGH_ORACLE_PRECISION, numerator);
    value /= denominator;
    value
}

const fn gate(passed: bool) -> &'static str {
    if passed { "pass" } else { "fail" }
}

fn candidate_category_tag(
    category: forward_backward_protocol::CandidateErrorCategory,
) -> &'static str {
    use forward_backward_protocol::CandidateErrorCategory;
    match category {
        CandidateErrorCategory::Validation => "validation",
        CandidateErrorCategory::Integration => "integration",
        CandidateErrorCategory::TailWork => "tail_work",
        CandidateErrorCategory::WorkCounterOverflow => "work_counter_overflow",
        CandidateErrorCategory::InvalidOutput => "invalid_output",
        CandidateErrorCategory::Other => "other",
    }
}

fn pass_tag(pass: forward_backward_protocol::Pass) -> &'static str {
    use forward_backward_protocol::Pass;
    match pass {
        Pass::Forward => "forward",
        Pass::Reverse => "reverse",
        Pass::Tail => "tail",
    }
}

fn source_category_tag(category: forward_backward_protocol::SourceCategory) -> &'static str {
    use forward_backward_protocol::SourceCategory;
    match category {
        SourceCategory::NonFiniteInput => "non_finite_input",
        SourceCategory::InvalidDeltaTime => "invalid_delta_time",
        SourceCategory::StepBudgetExceeded => "step_budget_exceeded",
        SourceCategory::StepSizeUnderflow => "step_size_underflow",
        SourceCategory::NonFiniteState => "non_finite_state",
        SourceCategory::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordScalar;
    use forward_backward_protocol::FilterOrder;

    #[test]
    fn endpoint_work_ceiling_accepts_the_boundary() {
        assert!(aggregate_work_passes(3, 896, 0, 6_272).unwrap());
        assert!(!aggregate_work_passes(3, 897, 0, 6_272).unwrap());
        assert!(!aggregate_work_passes(3, 896, 0, 6_273).unwrap());
    }

    #[test]
    fn omitted_tail_is_an_explicit_failure() {
        assert_eq!(
            tail_classification(&TailEvidence::Omitted),
            ("omitted", "fail", false)
        );
        assert_eq!(
            tail_classification(&TailEvidence::Analytic),
            ("analytic", "pass", true)
        );
    }

    #[test]
    fn fixed_slice_contract_remains_order_eight_f64() {
        let manifest = filtered_sentinel_manifest().unwrap();
        assert_eq!(manifest.selection().filter_order(), FilterOrder::Eight);
        assert_eq!(manifest.selection().scalar(), RecordScalar::F64);
        assert_eq!(manifest.entries().len(), 8);
    }
}
