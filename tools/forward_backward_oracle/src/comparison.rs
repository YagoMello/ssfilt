//! Private cross-cell acceptance comparisons for the response matrix.

use core::fmt;
use std::error::Error as StdError;

use forward_backward_protocol::{CandidateErrorResponse, CaseIdentity, FilterOrder};
use rug::Float;

use crate::assessment::{
    AssessmentError, HIGH_ORACLE_PRECISION, extract_original_knots, weighted_error,
};
use crate::candidate::NonFiniteSuccess;
use crate::identity::{CellVariant, IdentityError, ResponseCellKey, identity_for};
use crate::matrix::ResponseCellAssessment;
use crate::refinement::ScalarRefinement;
use crate::score::CandidateDisposition;
use crate::{CaseCoordinate, PhysicalCutoff, RecordScalar, RepresentedRecord};

const F64_PARTITION_RMS_LIMIT: (u32, u32) = (25, 1_000_000);
const F64_PARTITION_PEAK_LIMIT: (u32, u32) = (1, 10_000);
const F32_PARTITION_RMS_LIMIT: (u32, u32) = (1, 1_000);
const F32_PARTITION_PEAK_LIMIT: (u32, u32) = (4, 1_000);
const F64_PHYSICAL_SCALE_PEAK_LIMIT: (u32, u32) = (2, 1_000_000_000);
const F32_PHYSICAL_SCALE_PEAK_LIMIT: (u32, u32) = (2, 10_000);

/// The relationship checked by one cross-cell comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CrossCellComparison {
    PartitionInvariance,
    PhysicalScaling,
}
/// The role of a member in a cross-cell comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComparisonMember {
    Base,
    ScalarRefined,
    OneHz,
    ThirtySevenHz,
}

/// A normalized metric, its inclusive frozen limit, and the gate result.
#[derive(Clone, Debug)]
pub(crate) struct ComparisonMetricGate {
    measured: Float,
    limit: Float,
    passed: bool,
}

impl ComparisonMetricGate {
    #[must_use]
    pub(crate) const fn measured(&self) -> &Float {
        &self.measured
    }

    #[must_use]
    pub(crate) const fn limit(&self) -> &Float {
        &self.limit
    }

    #[must_use]
    pub(crate) const fn passed(&self) -> bool {
        self.passed
    }
}

/// Mutual partition metrics on the base record's interval weights and scale.
#[derive(Clone, Debug)]
pub(crate) struct PartitionComparisonMetrics {
    normalized_rms: ComparisonMetricGate,
    normalized_peak: ComparisonMetricGate,
}

impl PartitionComparisonMetrics {
    #[must_use]
    pub(crate) const fn normalized_rms(&self) -> &ComparisonMetricGate {
        &self.normalized_rms
    }

    #[must_use]
    pub(crate) const fn normalized_peak(&self) -> &ComparisonMetricGate {
        &self.normalized_peak
    }
}

/// Physical-scaling comparison metric; this contract gates peak error only.
#[derive(Clone, Debug)]
pub(crate) struct PhysicalScalingComparisonMetrics {
    normalized_peak: ComparisonMetricGate,
}

impl PhysicalScalingComparisonMetrics {
    #[must_use]
    pub(crate) const fn normalized_peak(&self) -> &ComparisonMetricGate {
        &self.normalized_peak
    }
}

/// A local member failure that makes a pair comparison ineligible.
#[derive(Clone, Debug)]
pub(crate) enum MemberIneligibilityReason {
    CandidateError(CandidateErrorResponse),
    NonFiniteSuccess(NonFiniteSuccess),
    WholeRecordCandidateGatesFailed {
        rms_passed: bool,
        peak_passed: bool,
    },
}

/// Ineligible member with the identity needed by a future typed report row.
#[derive(Clone, Debug)]
pub(crate) struct IneligibleMember {
    key: ResponseCellKey,
    reason: MemberIneligibilityReason,
}

impl IneligibleMember {
    #[must_use]
    pub(crate) const fn key(&self) -> ResponseCellKey {
        self.key
    }

    #[must_use]
    pub(crate) const fn reason(&self) -> &MemberIneligibilityReason {
        &self.reason
    }
}

/// Candidate failures are retained as cell outcomes so the matrix can continue.
#[derive(Clone, Debug)]
pub(crate) struct IneligibleComparison {
    first: Option<IneligibleMember>,
    second: Option<IneligibleMember>,
}

impl IneligibleComparison {
    #[must_use]
    pub(crate) const fn first(&self) -> Option<&IneligibleMember> {
        self.first.as_ref()
    }

    #[must_use]
    pub(crate) const fn second(&self) -> Option<&IneligibleMember> {
        self.second.as_ref()
    }
}

/// Whether a pair is measurable, or which member failures excluded it.
#[derive(Clone, Debug)]
pub(crate) enum CrossCellOutcome<T> {
    Eligible(T),
    Ineligible(Box<IneligibleComparison>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyField {
    Coordinate,
    Scalar,
    Cutoff,
    FilterOrder,
    Variant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyValue {
    Coordinate(CaseCoordinate),
    Scalar(RecordScalar),
    Cutoff(PhysicalCutoff),
    FilterOrder(FilterOrder),
    Variant(CellVariant),
}

/// Fatal mismatch or arithmetic failure while preparing a comparison.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CrossCellComparisonErrorKind {
    KeyMismatch {
        member: ComparisonMember,
        field: KeyField,
        expected: KeyValue,
        actual: KeyValue,
    },
    Identity {
        member: ComparisonMember,
        source: IdentityError,
    },
    IdentityMismatch {
        member: ComparisonMember,
        stored: CaseIdentity,
        recomputed: CaseIdentity,
    },
    OracleOrderMismatch {
        member: ComparisonMember,
        key_order: FilterOrder,
        oracle_order: usize,
    },
    RecordShapeMismatch {
        member: ComparisonMember,
        interval_count: usize,
        sample_count: usize,
    },
    PairShapeMismatch {
        first_member: ComparisonMember,
        first_interval_count: usize,
        first_sample_count: usize,
        second_member: ComparisonMember,
        second_interval_count: usize,
        second_sample_count: usize,
    },
    SampleMismatch {
        first_member: ComparisonMember,
        second_member: ComparisonMember,
        index: usize,
        first_bits: u64,
        second_bits: u64,
    },
    RefinementShapeOverflow {
        base_interval_count: usize,
    },
    RefinementOriginalSampleCountMismatch {
        expected: usize,
        actual: usize,
    },
    RefinementRecordShapeMismatch {
        expected_interval_count: usize,
        actual_interval_count: usize,
        expected_sample_count: usize,
        actual_sample_count: usize,
    },
    RefinementOriginalSampleMismatch {
        original_index: usize,
        refined_index: usize,
        base_bits: u64,
        refined_bits: u64,
    },
    RefinementIndexMapping {
        original_index: usize,
        mapped_index: Option<usize>,
        refined_length: usize,
    },
    CandidateOutputLengthMismatch {
        member: ComparisonMember,
        expected: usize,
        actual: usize,
    },
    CandidateOutputNonFinite {
        member: ComparisonMember,
        index: usize,
    },
    RefinementMapping(AssessmentError),
    Metric(AssessmentError),
}

/// Fatal errors retain both member keys so the failing pair remains identifiable.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CrossCellComparisonError {
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    kind: CrossCellComparisonErrorKind,
}

impl CrossCellComparisonError {
    #[must_use]
    pub(crate) const fn comparison(&self) -> CrossCellComparison {
        self.comparison
    }

    #[must_use]
    pub(crate) const fn first_key(&self) -> ResponseCellKey {
        self.first_key
    }

    #[must_use]
    pub(crate) const fn second_key(&self) -> ResponseCellKey {
        self.second_key
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> &CrossCellComparisonErrorKind {
        &self.kind
    }
}

impl fmt::Display for CrossCellComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} comparison between {:?} and {:?} failed: {:?}",
            self.comparison, self.first_key, self.second_key, self.kind
        )
    }
}

impl StdError for CrossCellComparisonError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.kind {
            CrossCellComparisonErrorKind::Identity { source, .. } => Some(source),
            CrossCellComparisonErrorKind::RefinementMapping(source)
            | CrossCellComparisonErrorKind::Metric(source) => Some(source),
            _ => None,
        }
    }
}

/// Compare base and scalar-refined candidates at their original timestamps.
///
/// Each wrapper is provenance-checked before candidate eligibility is examined.
/// Refined values are extracted only through the frozen original-index mapping,
/// and metrics use the base record's represented intervals and Q.
pub(crate) fn compare_partition_candidates(
    base_cell: &ResponseCellAssessment,
    base_record: &RepresentedRecord,
    refinement: &ScalarRefinement,
    refined_cell: &ResponseCellAssessment,
) -> Result<CrossCellOutcome<PartitionComparisonMetrics>, CrossCellComparisonError> {
    validate_partition_provenance(base_cell, base_record, refinement, refined_cell)?;
    validate_partition_members(base_cell, base_record, refinement, refined_cell)?;

    let first_key = base_cell.key();
    let second_key = refined_cell.key();
    if let Some(ineligible) = pair_ineligibility(base_cell, refined_cell) {
        return Ok(CrossCellOutcome::Ineligible(Box::new(ineligible)));
    }
    let refined_output = refined_cell.scored_output().expect("eligible candidate output");
    validate_output_values(
        CrossCellComparison::PartitionInvariance,
        first_key,
        second_key,
        ComparisonMember::ScalarRefined,
        refined_output,
    )?;

    let original_refined_values = extract_original_knots(refinement, refined_output).map_err(
        |error| {
            comparison_error(
                CrossCellComparison::PartitionInvariance,
                first_key,
                second_key,
                CrossCellComparisonErrorKind::RefinementMapping(error),
            )
        },
    )?;
    measure_partition_pair(
        base_cell,
        base_record,
        refined_cell,
        &original_refined_values,
    )
}

/// Apply the complete mutual metric and gates to already-aligned original knots.
/// Both production and narrow synthetic boundary tests pass through this path.
fn measure_partition_pair(
    base_cell: &ResponseCellAssessment,
    base_record: &RepresentedRecord,
    refined_cell: &ResponseCellAssessment,
    original_refined_values: &[Float],
) -> Result<CrossCellOutcome<PartitionComparisonMetrics>, CrossCellComparisonError> {
    if let Some(ineligible) = pair_ineligibility(base_cell, refined_cell) {
        return Ok(CrossCellOutcome::Ineligible(Box::new(ineligible)));
    }
    let first_key = base_cell.key();
    let second_key = refined_cell.key();
    let base_output = checked_scored_output(
        CrossCellComparison::PartitionInvariance,
        first_key,
        second_key,
        ComparisonMember::Base,
        base_record,
        base_cell,
    )?;
    validate_output_values(
        CrossCellComparison::PartitionInvariance,
        first_key,
        second_key,
        ComparisonMember::ScalarRefined,
        original_refined_values,
    )?;
    let metrics = weighted_error(
        base_record,
        base_output,
        original_refined_values,
        HIGH_ORACLE_PRECISION,
    )
    .map_err(|error| {
        comparison_error(
            CrossCellComparison::PartitionInvariance,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::Metric(error),
        )
    })?;
    let (rms_limit, peak_limit) = partition_limits(base_record.scalar());
    Ok(CrossCellOutcome::Eligible(PartitionComparisonMetrics {
        normalized_rms: metric_gate(metrics.normalized_rms(), rms_limit),
        normalized_peak: metric_gate(metrics.normalized_peak(), peak_limit),
    }))
}

/// Compare normalized 1 Hz and 37 Hz candidate outputs index-for-index.
///
/// The normalized peak is divided by the first record's stored-sample Q. RMS is
/// intentionally not a gate in the physical-scaling contract.
pub(crate) fn compare_physical_scaling_candidates(
    one_hz_cell: &ResponseCellAssessment,
    one_hz_record: &RepresentedRecord,
    thirty_seven_hz_cell: &ResponseCellAssessment,
    thirty_seven_hz_record: &RepresentedRecord,
) -> Result<CrossCellOutcome<PhysicalScalingComparisonMetrics>, CrossCellComparisonError> {
    validate_cell_provenance(
        CrossCellComparison::PhysicalScaling,
        one_hz_cell,
        one_hz_record,
        thirty_seven_hz_cell.key(),
        ComparisonMember::OneHz,
    )?;
    validate_cell_provenance(
        CrossCellComparison::PhysicalScaling,
        thirty_seven_hz_cell,
        thirty_seven_hz_record,
        one_hz_cell.key(),
        ComparisonMember::ThirtySevenHz,
    )?;
    validate_physical_scale_members(
        one_hz_cell,
        one_hz_record,
        thirty_seven_hz_cell,
        thirty_seven_hz_record,
    )?;

    if let Some(ineligible) = pair_ineligibility(one_hz_cell, thirty_seven_hz_cell) {
        return Ok(CrossCellOutcome::Ineligible(Box::new(ineligible)));
    }

    let first_key = one_hz_cell.key();
    let second_key = thirty_seven_hz_cell.key();
    let one_hz_output = checked_scored_output(
        CrossCellComparison::PhysicalScaling,
        first_key,
        second_key,
        ComparisonMember::OneHz,
        one_hz_record,
        one_hz_cell,
    )?;
    let thirty_seven_hz_output = checked_scored_output(
        CrossCellComparison::PhysicalScaling,
        first_key,
        second_key,
        ComparisonMember::ThirtySevenHz,
        thirty_seven_hz_record,
        thirty_seven_hz_cell,
    )?;

    let metrics = weighted_error(
        one_hz_record,
        one_hz_output,
        thirty_seven_hz_output,
        HIGH_ORACLE_PRECISION,
    )
    .map_err(|error| {
        comparison_error(
            CrossCellComparison::PhysicalScaling,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::Metric(error),
        )
    })?;
    let peak_limit = physical_scale_peak_limit(one_hz_record.scalar());

    Ok(CrossCellOutcome::Eligible(
        PhysicalScalingComparisonMetrics {
            normalized_peak: metric_gate(metrics.normalized_peak(), peak_limit),
        },
    ))
}

fn validate_partition_provenance(
    base_cell: &ResponseCellAssessment,
    base_record: &RepresentedRecord,
    refinement: &ScalarRefinement,
    refined_cell: &ResponseCellAssessment,
) -> Result<(), CrossCellComparisonError> {
    let comparison = CrossCellComparison::PartitionInvariance;
    let first_key = base_cell.key();
    let second_key = refined_cell.key();
    validate_cell_provenance(
        comparison,
        base_cell,
        base_record,
        second_key,
        ComparisonMember::Base,
    )?;
    validate_cell_provenance(
        comparison,
        refined_cell,
        refinement.record(),
        first_key,
        ComparisonMember::ScalarRefined,
    )
}

fn validate_cell_provenance(
    comparison: CrossCellComparison,
    cell: &ResponseCellAssessment,
    record: &RepresentedRecord,
    other_key: ResponseCellKey,
    member: ComparisonMember,
) -> Result<(), CrossCellComparisonError> {
    let key = cell.key();
    let (first_key, second_key) = match member {
        ComparisonMember::Base | ComparisonMember::OneHz => (key, other_key),
        ComparisonMember::ScalarRefined | ComparisonMember::ThirtySevenHz => (other_key, key),
    };
    let recomputed = identity_for(&key, record).map_err(|error| {
        comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::Identity {
                member,
                source: error,
            },
        )
    })?;
    if cell.identity() != recomputed {
        return Err(comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::IdentityMismatch {
                member,
                stored: cell.identity(),
                recomputed,
            },
        ));
    }
    let expected_order = numeric_order(key.filter_order());
    let oracle_order = cell.assessment().oracle().order();
    if oracle_order != expected_order {
        return Err(comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::OracleOrderMismatch {
                member,
                key_order: key.filter_order(),
                oracle_order,
            },
        ));
    }
    Ok(())
}

fn validate_partition_members(
    base_cell: &ResponseCellAssessment,
    base_record: &RepresentedRecord,
    refinement: &ScalarRefinement,
    refined_cell: &ResponseCellAssessment,
) -> Result<(), CrossCellComparisonError> {
    let comparison = CrossCellComparison::PartitionInvariance;
    let base_key = base_cell.key();
    let refined_key = refined_cell.key();
    require_key_value(
        comparison,
        base_key,
        refined_key,
        ComparisonMember::Base,
        KeyField::Variant,
        KeyValue::Variant(CellVariant::Base),
        KeyValue::Variant(base_key.variant()),
    )?;
    require_key_value(
        comparison,
        base_key,
        refined_key,
        ComparisonMember::ScalarRefined,
        KeyField::Variant,
        KeyValue::Variant(CellVariant::ScalarRefined),
        KeyValue::Variant(refined_key.variant()),
    )?;
    for (field, first, second) in [
        (
            KeyField::Coordinate,
            KeyValue::Coordinate(base_key.coordinate()),
            KeyValue::Coordinate(refined_key.coordinate()),
        ),
        (
            KeyField::Scalar,
            KeyValue::Scalar(base_key.scalar()),
            KeyValue::Scalar(refined_key.scalar()),
        ),
        (
            KeyField::Cutoff,
            KeyValue::Cutoff(base_key.cutoff()),
            KeyValue::Cutoff(refined_key.cutoff()),
        ),
        (
            KeyField::FilterOrder,
            KeyValue::FilterOrder(base_key.filter_order()),
            KeyValue::FilterOrder(refined_key.filter_order()),
        ),
    ] {
        require_key_value(
            comparison,
            base_key,
            refined_key,
            ComparisonMember::ScalarRefined,
            field,
            first,
            second,
        )?;
    }
    if refinement.coordinate() != base_key.coordinate() {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::KeyMismatch {
                member: ComparisonMember::ScalarRefined,
                field: KeyField::Coordinate,
                expected: KeyValue::Coordinate(base_key.coordinate()),
                actual: KeyValue::Coordinate(refinement.coordinate()),
            },
        ));
    }
    if refinement.physical_cutoff() != base_key.cutoff() {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::KeyMismatch {
                member: ComparisonMember::ScalarRefined,
                field: KeyField::Cutoff,
                expected: KeyValue::Cutoff(base_key.cutoff()),
                actual: KeyValue::Cutoff(refinement.physical_cutoff()),
            },
        ));
    }

    validate_record_shape(comparison, base_key, refined_key, ComparisonMember::Base, base_record)?;
    let refined_record = refinement.record();
    validate_record_shape(
        comparison,
        base_key,
        refined_key,
        ComparisonMember::ScalarRefined,
        refined_record,
    )?;
    if refinement.original_sample_count() != base_record.samples().len() {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::RefinementOriginalSampleCountMismatch {
                expected: base_record.samples().len(),
                actual: refinement.original_sample_count(),
            },
        ));
    }
    let Some(expected_refined_intervals) = base_record.intervals_seconds().len().checked_mul(3)
    else {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::RefinementShapeOverflow {
                base_interval_count: base_record.intervals_seconds().len(),
            },
        ));
    };
    let Some(expected_refined_samples) = expected_refined_intervals.checked_add(1) else {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::RefinementShapeOverflow {
                base_interval_count: base_record.intervals_seconds().len(),
            },
        ));
    };
    if refined_record.intervals_seconds().len() != expected_refined_intervals
        || refined_record.samples().len() != expected_refined_samples
    {
        return Err(comparison_error(
            comparison,
            base_key,
            refined_key,
            CrossCellComparisonErrorKind::RefinementRecordShapeMismatch {
                expected_interval_count: expected_refined_intervals,
                actual_interval_count: refined_record.intervals_seconds().len(),
                expected_sample_count: expected_refined_samples,
                actual_sample_count: refined_record.samples().len(),
            },
        ));
    }
    for (original_index, base_sample) in base_record.samples().iter().enumerate() {
        let mapped_index = refinement.refined_index_for_original(original_index);
        let Some(refined_index) = mapped_index.filter(|index| *index < refined_record.samples().len())
        else {
            return Err(comparison_error(
                comparison,
                base_key,
                refined_key,
                CrossCellComparisonErrorKind::RefinementIndexMapping {
                    original_index,
                    mapped_index,
                    refined_length: refined_record.samples().len(),
                },
            ));
        };
        let refined_sample = refined_record.samples()[refined_index];
        if base_sample.to_bits() != refined_sample.to_bits() {
            return Err(comparison_error(
                comparison,
                base_key,
                refined_key,
                CrossCellComparisonErrorKind::RefinementOriginalSampleMismatch {
                    original_index,
                    refined_index,
                    base_bits: base_sample.to_bits(),
                    refined_bits: refined_sample.to_bits(),
                },
            ));
        }
    }
    Ok(())
}

fn validate_physical_scale_members(
    one_hz_cell: &ResponseCellAssessment,
    one_hz_record: &RepresentedRecord,
    thirty_seven_hz_cell: &ResponseCellAssessment,
    thirty_seven_hz_record: &RepresentedRecord,
) -> Result<(), CrossCellComparisonError> {
    let comparison = CrossCellComparison::PhysicalScaling;
    let one_hz_key = one_hz_cell.key();
    let thirty_seven_hz_key = thirty_seven_hz_cell.key();
    require_key_value(
        comparison,
        one_hz_key,
        thirty_seven_hz_key,
        ComparisonMember::OneHz,
        KeyField::Cutoff,
        KeyValue::Cutoff(PhysicalCutoff::OneHz),
        KeyValue::Cutoff(one_hz_key.cutoff()),
    )?;
    require_key_value(
        comparison,
        one_hz_key,
        thirty_seven_hz_key,
        ComparisonMember::ThirtySevenHz,
        KeyField::Cutoff,
        KeyValue::Cutoff(PhysicalCutoff::ThirtySevenHz),
        KeyValue::Cutoff(thirty_seven_hz_key.cutoff()),
    )?;
    for (field, first, second) in [
        (
            KeyField::Coordinate,
            KeyValue::Coordinate(one_hz_key.coordinate()),
            KeyValue::Coordinate(thirty_seven_hz_key.coordinate()),
        ),
        (
            KeyField::Scalar,
            KeyValue::Scalar(one_hz_key.scalar()),
            KeyValue::Scalar(thirty_seven_hz_key.scalar()),
        ),
        (
            KeyField::FilterOrder,
            KeyValue::FilterOrder(one_hz_key.filter_order()),
            KeyValue::FilterOrder(thirty_seven_hz_key.filter_order()),
        ),
        (
            KeyField::Variant,
            KeyValue::Variant(one_hz_key.variant()),
            KeyValue::Variant(thirty_seven_hz_key.variant()),
        ),
    ] {
        require_key_value(
            comparison,
            one_hz_key,
            thirty_seven_hz_key,
            ComparisonMember::ThirtySevenHz,
            field,
            first,
            second,
        )?;
    }
    validate_record_shape(
        comparison,
        one_hz_key,
        thirty_seven_hz_key,
        ComparisonMember::OneHz,
        one_hz_record,
    )?;
    validate_record_shape(
        comparison,
        one_hz_key,
        thirty_seven_hz_key,
        ComparisonMember::ThirtySevenHz,
        thirty_seven_hz_record,
    )?;
    if one_hz_record.intervals_seconds().len() != thirty_seven_hz_record.intervals_seconds().len()
        || one_hz_record.samples().len() != thirty_seven_hz_record.samples().len()
    {
        return Err(comparison_error(
            comparison,
            one_hz_key,
            thirty_seven_hz_key,
            CrossCellComparisonErrorKind::PairShapeMismatch {
                first_member: ComparisonMember::OneHz,
                first_interval_count: one_hz_record.intervals_seconds().len(),
                first_sample_count: one_hz_record.samples().len(),
                second_member: ComparisonMember::ThirtySevenHz,
                second_interval_count: thirty_seven_hz_record.intervals_seconds().len(),
                second_sample_count: thirty_seven_hz_record.samples().len(),
            },
        ));
    }
    for index in 0..one_hz_record.samples().len() {
        let one_bits = one_hz_record.samples()[index].to_bits();
        let thirty_seven_bits = thirty_seven_hz_record.samples()[index].to_bits();
        if one_bits != thirty_seven_bits {
            return Err(comparison_error(
                comparison,
                one_hz_key,
                thirty_seven_hz_key,
                CrossCellComparisonErrorKind::SampleMismatch {
                    first_member: ComparisonMember::OneHz,
                    second_member: ComparisonMember::ThirtySevenHz,
                    index,
                    first_bits: one_bits,
                    second_bits: thirty_seven_bits,
                },
            ));
        }
    }
    Ok(())
}

fn validate_record_shape(
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    member: ComparisonMember,
    record: &RepresentedRecord,
) -> Result<(), CrossCellComparisonError> {
    if record.samples().is_empty()
        || record.intervals_seconds().len().checked_add(1) != Some(record.samples().len())
    {
        return Err(comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::RecordShapeMismatch {
                member,
                interval_count: record.intervals_seconds().len(),
                sample_count: record.samples().len(),
            },
        ));
    }
    Ok(())
}

fn require_key_value(
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    member: ComparisonMember,
    field: KeyField,
    expected: KeyValue,
    actual: KeyValue,
) -> Result<(), CrossCellComparisonError> {
    if expected == actual {
        return Ok(());
    }
    Err(comparison_error(
        comparison,
        first_key,
        second_key,
        CrossCellComparisonErrorKind::KeyMismatch {
            member,
            field,
            expected,
            actual,
        },
    ))
}

fn pair_ineligibility(
    first_cell: &ResponseCellAssessment,
    second_cell: &ResponseCellAssessment,
) -> Option<IneligibleComparison> {
    let first = member_ineligibility(first_cell);
    let second = member_ineligibility(second_cell);
    (first.is_some() || second.is_some()).then_some(IneligibleComparison { first, second })
}

fn member_ineligibility(cell: &ResponseCellAssessment) -> Option<IneligibleMember> {
    let reason = match cell.assessment().candidate() {
        CandidateDisposition::Scored { gates, .. } if !gates.passed() => {
            Some(MemberIneligibilityReason::WholeRecordCandidateGatesFailed {
                rms_passed: gates.rms().passed(),
                peak_passed: gates.peak().passed(),
            })
        }
        CandidateDisposition::Scored { .. } => None,
        CandidateDisposition::CandidateError(error) => {
            Some(MemberIneligibilityReason::CandidateError(error.clone()))
        }
        CandidateDisposition::NonFiniteSuccess(error) => {
            Some(MemberIneligibilityReason::NonFiniteSuccess(error.clone()))
        }
    }?;
    Some(IneligibleMember {
        key: cell.key(),
        reason,
    })
}

fn checked_scored_output<'a>(
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    member: ComparisonMember,
    record: &RepresentedRecord,
    cell: &'a ResponseCellAssessment,
) -> Result<&'a [Float], CrossCellComparisonError> {
    let output = cell
        .scored_output()
        .expect("eligible candidate must retain its scored output");
    if output.len() != record.samples().len() {
        return Err(comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::CandidateOutputLengthMismatch {
                member,
                expected: record.samples().len(),
                actual: output.len(),
            },
        ));
    }
    validate_output_values(comparison, first_key, second_key, member, output)?;
    Ok(output)
}

fn validate_output_values(
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    member: ComparisonMember,
    output: &[Float],
) -> Result<(), CrossCellComparisonError> {
    if let Some(index) = output.iter().position(|value| !value.is_finite()) {
        return Err(comparison_error(
            comparison,
            first_key,
            second_key,
            CrossCellComparisonErrorKind::CandidateOutputNonFinite { member, index },
        ));
    }
    Ok(())
}

fn partition_limits(scalar: RecordScalar) -> (Float, Float) {
    let (rms, peak) = match scalar {
        RecordScalar::F64 => (F64_PARTITION_RMS_LIMIT, F64_PARTITION_PEAK_LIMIT),
        RecordScalar::F32 => (F32_PARTITION_RMS_LIMIT, F32_PARTITION_PEAK_LIMIT),
    };
    (decimal_rational(rms), decimal_rational(peak))
}

fn physical_scale_peak_limit(scalar: RecordScalar) -> Float {
    match scalar {
        RecordScalar::F64 => decimal_rational(F64_PHYSICAL_SCALE_PEAK_LIMIT),
        RecordScalar::F32 => decimal_rational(F32_PHYSICAL_SCALE_PEAK_LIMIT),
    }
}

fn decimal_rational((numerator, denominator): (u32, u32)) -> Float {
    let mut value = Float::with_val(HIGH_ORACLE_PRECISION, numerator);
    value /= denominator;
    value
}

fn metric_gate(measured: &Float, limit: Float) -> ComparisonMetricGate {
    ComparisonMetricGate {
        passed: measured <= &limit,
        measured: measured.clone(),
        limit,
    }
}

fn numeric_order(order: FilterOrder) -> usize {
    match order {
        FilterOrder::One => 1,
        FilterOrder::Two => 2,
        FilterOrder::Four => 4,
        FilterOrder::Eight => 8,
    }
}

fn comparison_error(
    comparison: CrossCellComparison,
    first_key: ResponseCellKey,
    second_key: ResponseCellKey,
    kind: CrossCellComparisonErrorKind,
) -> CrossCellComparisonError {
    CrossCellComparisonError {
        comparison,
        first_key,
        second_key,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use forward_backward_protocol::{
        CandidateErrorCategory, CandidateErrorResponse, CaseResult, OutputBits, Pass,
        SuccessResponse, TailEvidence,
    };
    use rug::float::Round;

    use crate::matrix::{ResponseCellAssessment, score_response};
    use crate::{
        QMax, ScheduleKind, ToneCoordinate, ToneFrequency, ToneOrder, TonePhase,
        build_nominal_tone_record, build_represented_tone_case, evaluate, refine_represented_case,
    };

    fn tone_coordinate(schedule: ScheduleKind) -> ToneCoordinate {
        ToneCoordinate {
            order: ToneOrder::One,
            frequency: ToneFrequency::Two,
            phase: TonePhase::SeventeenHundredths,
            q_max: QMax::PiOver32,
            schedule,
        }
    }

    fn tone_case(scalar: RecordScalar, cutoff: PhysicalCutoff, schedule: ScheduleKind) -> crate::RepresentedCase {
        let nominal = build_nominal_tone_record(tone_coordinate(schedule)).unwrap();
        build_represented_tone_case(&nominal, scalar, cutoff).unwrap()
    }

    fn tone_key(
        scalar: RecordScalar,
        cutoff: PhysicalCutoff,
        variant: CellVariant,
        schedule: ScheduleKind,
    ) -> ResponseCellKey {
        ResponseCellKey::new(
            CaseCoordinate::Tone(tone_coordinate(schedule)),
            scalar,
            cutoff,
            FilterOrder::One,
            variant,
        )
    }

    fn key_for_simple_physical(
        scalar: RecordScalar,
        cutoff: PhysicalCutoff,
    ) -> ResponseCellKey {
        // The semantic coordinate is shared by both members; these compact
        // test records focus on the pairwise stored-sample normalization.
        tone_key(scalar, cutoff, CellVariant::Base, ScheduleKind::Uniform)
    }

    fn success_for(key: &ResponseCellKey, record: &RepresentedRecord, bits: OutputBits) -> CaseResult {
        CaseResult::Success(SuccessResponse {
            identity: identity_for(key, record).unwrap(),
            output_bits: bits,
            tail_evidence: TailEvidence::Omitted,
            forward: Default::default(),
            tail: Default::default(),
            reverse: Default::default(),
        })
    }

    fn candidate_error_for(key: &ResponseCellKey, record: &RepresentedRecord) -> CaseResult {
        CaseResult::CandidateError(CandidateErrorResponse {
            identity: identity_for(key, record).unwrap(),
            category: CandidateErrorCategory::Integration,
            pass: Some(Pass::Reverse),
            interval: Some(0),
            source_category: None,
        })
    }

    fn bits_from_values(scalar: RecordScalar, values: &[Float]) -> OutputBits {
        match scalar {
            RecordScalar::F32 => OutputBits::F32(
                values
                    .iter()
                    .map(|value| value.to_f32_round(Round::Nearest).to_bits())
                    .collect(),
            ),
            RecordScalar::F64 => OutputBits::F64(
                values
                    .iter()
                    .map(|value| value.to_f64_round(Round::Nearest).to_bits())
                    .collect(),
            ),
        }
    }

    fn oracle_bits(key: &ResponseCellKey, record: &RepresentedRecord) -> OutputBits {
        bits_from_values(
            record.scalar(),
            evaluate(numeric_order(key.filter_order()), record, HIGH_ORACLE_PRECISION).values(),
        )
    }

    fn cell_with_bits(
        key: ResponseCellKey,
        record: &RepresentedRecord,
        bits: OutputBits,
        output_override: Option<Vec<Float>>,
    ) -> ResponseCellAssessment {
        let assessment = score_response(
            key.filter_order(),
            record,
            success_for(&key, record, bits),
        )
        .unwrap();
        ResponseCellAssessment::from_test(
            key,
            identity_for(&key, record).unwrap(),
            assessment,
            output_override,
        )
    }

    fn passing_cell(key: ResponseCellKey, record: &RepresentedRecord) -> ResponseCellAssessment {
        let cell = cell_with_bits(key, record, oracle_bits(&key, record), None);
        let CandidateDisposition::Scored { gates, .. } = cell.assessment().candidate() else {
            panic!("the oracle output should be finite");
        };
        assert!(gates.passed(), "test member must pass its local gates");
        cell
    }

    fn simple_record(scalar: RecordScalar, cutoff: PhysicalCutoff, intervals: &[f64]) -> RepresentedRecord {
        let mut samples = vec![0.0, 1.0];
        samples.resize(intervals.len() + 1, 1.0);
        match scalar {
            RecordScalar::F64 => RepresentedRecord::from_f64(cutoff.hz(), intervals.to_vec(), samples).unwrap(),
            RecordScalar::F32 => RepresentedRecord::from_f32(
                cutoff.hz() as f32,
                intervals.iter().map(|value| *value as f32).collect(),
                samples.iter().map(|value| *value as f32).collect(),
            )
            .unwrap(),
        }
    }

    fn zeros(length: usize) -> Vec<Float> {
        (0..length)
            .map(|_| Float::with_val(HIGH_ORACLE_PRECISION, 0))
            .collect()
    }

    fn make_partition_members(
        scalar: RecordScalar,
        schedule: ScheduleKind,
    ) -> (
        crate::RepresentedCase,
        ScalarRefinement,
        ResponseCellAssessment,
        ResponseCellAssessment,
    ) {
        let represented = tone_case(scalar, PhysicalCutoff::OneHz, schedule);
        let refinement = refine_represented_case(&represented).unwrap();
        let base_record = represented.record();
        let base_key = tone_key(scalar, PhysicalCutoff::OneHz, CellVariant::Base, schedule);
        let refined_key = tone_key(
            scalar,
            PhysicalCutoff::OneHz,
            CellVariant::ScalarRefined,
            schedule,
        );
        let base_cell = passing_cell(base_key, base_record);
        let refined_cell = passing_cell(refined_key, refinement.record());
        (represented, refinement, base_cell, refined_cell)
    }

    #[test]
    fn partition_limits_are_reached_and_exceeded_by_one_256_bit_step() {
        for (scalar, rms_limit, peak_limit) in [
            (RecordScalar::F64, (25, 1_000_000), (1, 10_000)),
            (RecordScalar::F32, (1, 1_000), (4, 1_000)),
        ] {
            let base_record = match scalar {
                RecordScalar::F64 => {
                    RepresentedRecord::from_f64(1.0, vec![1.0], vec![0.0, 1.0]).unwrap()
                }
                RecordScalar::F32 => {
                    RepresentedRecord::from_f32(1.0, vec![1.0], vec![0.0, 1.0]).unwrap()
                }
            };
            let base_key = tone_key(
                scalar,
                PhysicalCutoff::OneHz,
                CellVariant::Base,
                ScheduleKind::Uniform,
            );
            let refined_key = tone_key(
                scalar,
                PhysicalCutoff::OneHz,
                CellVariant::ScalarRefined,
                ScheduleKind::Uniform,
            );
            let mut base_cell = passing_cell(base_key, &base_record);
            let mut refined_cell = passing_cell(refined_key, &base_record);
            base_cell.set_test_output_override(zeros(base_record.samples().len()));
            let rms_limit = decimal_rational(rms_limit);
            let peak_limit = decimal_rational(peak_limit);

            for (target, use_rms) in [(&rms_limit, true), (&peak_limit, false)] {
                let errors = if use_rms {
                    vec![target.clone(), target.clone()]
                } else {
                    vec![target.clone(), Float::with_val(HIGH_ORACLE_PRECISION, 0)]
                };
                refined_cell.set_test_output_override(errors.clone());
                let CrossCellOutcome::Eligible(at_limit) = measure_partition_pair(
                    &base_cell,
                    &base_record,
                    &refined_cell,
                    &errors,
                )
                .unwrap()
                else {
                    panic!("locally passing members must be eligible");
                };
                let gate = if use_rms {
                    at_limit.normalized_rms()
                } else {
                    at_limit.normalized_peak()
                };
                assert_eq!(gate.measured(), target, "{scalar:?} equality");
                assert_eq!(gate.limit(), target);
                assert!(gate.passed(), "the frozen boundary is inclusive");

                let mut just_over = target.clone();
                just_over.next_up();
                let errors = if use_rms {
                    vec![just_over.clone(), just_over.clone()]
                } else {
                    vec![just_over.clone(), Float::with_val(HIGH_ORACLE_PRECISION, 0)]
                };
                refined_cell.set_test_output_override(errors.clone());
                let CrossCellOutcome::Eligible(over_limit) = measure_partition_pair(
                    &base_cell,
                    &base_record,
                    &refined_cell,
                    &errors,
                )
                .unwrap()
                else {
                    panic!("locally passing members must remain eligible");
                };
                let gate = if use_rms {
                    over_limit.normalized_rms()
                } else {
                    over_limit.normalized_peak()
                };
                assert_eq!(gate.measured(), &just_over, "{scalar:?} one 256-bit step");
                assert!(!gate.passed(), "one 256-bit step over must fail");
            }
        }
    }

    #[test]
    fn partition_compares_only_original_indices_and_uses_base_trapezoidal_weights() {
        let (represented, refinement, mut base_cell, mut refined_cell) =
            make_partition_members(RecordScalar::F64, ScheduleKind::Uneven);
        let base_record = represented.record();
        assert!(base_record
            .intervals_seconds()
            .windows(2)
            .any(|pair| pair[0].to_bits() != pair[1].to_bits()));

        let mut base_output = zeros(base_record.samples().len());
        let mut refined_output = zeros(refinement.record().samples().len());
        for (index, value) in base_output.iter_mut().enumerate() {
            *value = Float::with_val(HIGH_ORACLE_PRECISION, index as u32 + 1);
            let refined_index = refinement.refined_index_for_original(index).unwrap();
            refined_output[refined_index] = value.clone();
        }
        // Inserted values deliberately disagree; they must not affect the
        // original-timestamp mutual metric.
        for (index, value) in refined_output.iter_mut().enumerate() {
            if index % 3 != 0 {
                *value = Float::with_val(HIGH_ORACLE_PRECISION, 10_000);
            }
        }
        base_cell.set_test_output_override(base_output.clone());
        refined_cell.set_test_output_override(refined_output);

        let CrossCellOutcome::Eligible(metrics) = compare_partition_candidates(
            &base_cell,
            base_record,
            &refinement,
            &refined_cell,
        )
        .unwrap()
        else {
            panic!("both local candidates pass");
        };
        assert_eq!(metrics.normalized_rms().measured(), &Float::with_val(256, 0));
        assert_eq!(metrics.normalized_peak().measured(), &Float::with_val(256, 0));

        let mut differing_originals = zeros(refinement.record().samples().len());
        differing_originals[refinement.refined_index_for_original(0).unwrap()] =
            Float::with_val(HIGH_ORACLE_PRECISION, 1);
        let extracted = extract_original_knots(&refinement, &differing_originals).unwrap();
        base_cell.set_test_output_override(zeros(base_record.samples().len()));
        let expected = weighted_error(
            base_record,
            base_cell.scored_output().unwrap(),
            &extracted,
            HIGH_ORACLE_PRECISION,
        )
        .unwrap();
        let mut unweighted_sum = Float::with_val(HIGH_ORACLE_PRECISION, 0);
        for value in &extracted {
            unweighted_sum += Float::with_val(HIGH_ORACLE_PRECISION, value * value);
        }
        unweighted_sum /= extracted.len();
        unweighted_sum.sqrt_mut();
        unweighted_sum /= base_record.scale(HIGH_ORACLE_PRECISION);
        let unweighted = unweighted_sum;
        refined_cell.set_test_output_override(differing_originals);
        let CrossCellOutcome::Eligible(weighted) = compare_partition_candidates(
            &base_cell,
            base_record,
            &refinement,
            &refined_cell,
        )
        .unwrap()
        else {
            panic!("both local candidates pass");
        };
        assert_eq!(weighted.normalized_rms().measured(), expected.normalized_rms());
        assert_ne!(weighted.normalized_rms().measured(), &unweighted);
    }

    #[test]
    fn physical_scaling_peak_gate_is_inclusive_and_rejects_one_256_bit_step() {
        for (scalar, limit) in [
            (RecordScalar::F64, (2, 1_000_000_000)),
            (RecordScalar::F32, (2, 10_000)),
        ] {
            let one_record = simple_record(scalar, PhysicalCutoff::OneHz, &[0.25]);
            let thirty_seven_record = simple_record(scalar, PhysicalCutoff::ThirtySevenHz, &[0.125]);
            let one_key = key_for_simple_physical(scalar, PhysicalCutoff::OneHz);
            let thirty_seven_key = key_for_simple_physical(scalar, PhysicalCutoff::ThirtySevenHz);
            let mut one_cell = passing_cell(one_key, &one_record);
            let mut thirty_seven_cell = passing_cell(thirty_seven_key, &thirty_seven_record);
            one_cell.set_test_output_override(zeros(one_record.samples().len()));
            let limit = decimal_rational(limit);
            let mut exact = zeros(thirty_seven_record.samples().len());
            exact[0] = limit.clone();
            thirty_seven_cell.set_test_output_override(exact);

            let CrossCellOutcome::Eligible(at_limit) = compare_physical_scaling_candidates(
                &one_cell,
                &one_record,
                &thirty_seven_cell,
                &thirty_seven_record,
            )
            .unwrap()
            else {
                panic!("locally passing members must be eligible");
            };
            assert_eq!(at_limit.normalized_peak().measured(), &limit);
            assert_eq!(at_limit.normalized_peak().limit(), &limit);
            assert!(at_limit.normalized_peak().passed());

            let mut just_over = limit.clone();
            just_over.next_up();
            let mut over = zeros(thirty_seven_record.samples().len());
            over[0] = just_over.clone();
            thirty_seven_cell.set_test_output_override(over);
            let CrossCellOutcome::Eligible(over_limit) = compare_physical_scaling_candidates(
                &one_cell,
                &one_record,
                &thirty_seven_cell,
                &thirty_seven_record,
            )
            .unwrap()
            else {
                panic!("locally passing members remain eligible");
            };
            assert_eq!(over_limit.normalized_peak().measured(), &just_over);
            assert!(!over_limit.normalized_peak().passed());
        }
    }

    #[test]
    fn local_candidate_failures_are_collectable_only_after_provenance_checks() {
        let (represented, refinement, _base_cell, refined_cell) =
            make_partition_members(RecordScalar::F64, ScheduleKind::Uniform);
        let base_record = represented.record();
        let base_key = tone_key(
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            CellVariant::Base,
            ScheduleKind::Uniform,
        );

        let candidate_error = score_response(
            base_key.filter_order(),
            base_record,
            candidate_error_for(&base_key, base_record),
        )
        .unwrap();
        let error_cell = ResponseCellAssessment::from_test(
            base_key,
            identity_for(&base_key, base_record).unwrap(),
            candidate_error,
            None,
        );
        let CrossCellOutcome::Ineligible(ineligible) = compare_partition_candidates(
            &error_cell,
            base_record,
            &refinement,
            &refined_cell,
        )
        .unwrap()
        else {
            panic!("candidate error excludes the pair");
        };
        assert!(matches!(
            ineligible.first().unwrap().reason(),
            MemberIneligibilityReason::CandidateError(_)
        ));

        let mut nonfinite_bits = oracle_bits(&base_key, base_record);
        match &mut nonfinite_bits {
            OutputBits::F32(values) => values[0] = f32::NAN.to_bits(),
            OutputBits::F64(values) => values[0] = f64::NAN.to_bits(),
        }
        let nonfinite_assessment = score_response(
            base_key.filter_order(),
            base_record,
            success_for(&base_key, base_record, nonfinite_bits),
        )
        .unwrap();
        let nonfinite_cell = ResponseCellAssessment::from_test(
            base_key,
            identity_for(&base_key, base_record).unwrap(),
            nonfinite_assessment,
            None,
        );
        let CrossCellOutcome::Ineligible(ineligible) = compare_partition_candidates(
            &nonfinite_cell,
            base_record,
            &refinement,
            &refined_cell,
        )
        .unwrap()
        else {
            panic!("non-finite success excludes the pair");
        };
        assert!(matches!(
            ineligible.first().unwrap().reason(),
            MemberIneligibilityReason::NonFiniteSuccess(_)
        ));

        let gate_failure_bits = match base_key.scalar() {
            RecordScalar::F32 => OutputBits::F32(
                vec![1_000.0_f32.to_bits(); base_record.samples().len()],
            ),
            RecordScalar::F64 => OutputBits::F64(
                vec![1_000.0_f64.to_bits(); base_record.samples().len()],
            ),
        };
        let gate_assessment = score_response(
            base_key.filter_order(),
            base_record,
            success_for(&base_key, base_record, gate_failure_bits),
        )
        .unwrap();
        let gate_cell = ResponseCellAssessment::from_test(
            base_key,
            identity_for(&base_key, base_record).unwrap(),
            gate_assessment,
            None,
        );
        let CrossCellOutcome::Ineligible(ineligible) = compare_partition_candidates(
            &gate_cell,
            base_record,
            &refinement,
            &refined_cell,
        )
        .unwrap()
        else {
            panic!("failed local gates exclude the pair");
        };
        assert!(matches!(
            ineligible.first().unwrap().reason(),
            MemberIneligibilityReason::WholeRecordCandidateGatesFailed { .. }
        ));

        let changed_record = RepresentedRecord::from_f64(
            base_record.cutoff_hz(),
            base_record.intervals_seconds().to_vec(),
            base_record
                .samples()
                .iter()
                .enumerate()
                .map(|(index, value)| if index == 0 { value + 0.25 } else { *value })
                .collect(),
        )
        .unwrap();
        let fatal = compare_partition_candidates(
            &error_cell,
            &changed_record,
            &refinement,
            &refined_cell,
        )
        .unwrap_err();
        assert!(matches!(
            fatal.kind(),
            CrossCellComparisonErrorKind::IdentityMismatch { .. }
        ));
        assert_eq!(fatal.first_key(), base_key);
        assert_eq!(fatal.second_key(), refined_cell.key());
    }

    #[test]
    fn provenance_rejects_swapped_records_orders_and_cell_wrappers_with_context() {
        let one_record = simple_record(RecordScalar::F64, PhysicalCutoff::OneHz, &[0.25]);
        let thirty_seven_record = simple_record(
            RecordScalar::F64,
            PhysicalCutoff::ThirtySevenHz,
            &[0.125],
        );
        let one_key = key_for_simple_physical(RecordScalar::F64, PhysicalCutoff::OneHz);
        let thirty_seven_key = key_for_simple_physical(RecordScalar::F64, PhysicalCutoff::ThirtySevenHz);
        let one_cell = passing_cell(one_key, &one_record);
        let thirty_seven_cell = passing_cell(thirty_seven_key, &thirty_seven_record);

        let swapped_record = compare_physical_scaling_candidates(
            &one_cell,
            &thirty_seven_record,
            &thirty_seven_cell,
            &thirty_seven_record,
        )
        .unwrap_err();
        assert!(matches!(
            swapped_record.kind(),
            CrossCellComparisonErrorKind::Identity { .. }
        ));
        assert_eq!(swapped_record.first_key(), one_key);
        assert_eq!(swapped_record.second_key(), thirty_seven_key);

        let changed_same_cutoff = RepresentedRecord::from_f64(
            1.0,
            one_record.intervals_seconds().to_vec(),
            vec![0.25, 1.0],
        )
        .unwrap();
        let digest_error = compare_physical_scaling_candidates(
            &one_cell,
            &changed_same_cutoff,
            &thirty_seven_cell,
            &thirty_seven_record,
        )
        .unwrap_err();
        assert!(matches!(
            digest_error.kind(),
            CrossCellComparisonErrorKind::IdentityMismatch { .. }
        ));
        assert_eq!(digest_error.first_key(), one_key);
        assert_eq!(digest_error.second_key(), thirty_seven_key);

        let wrong_order_assessment = score_response(
            FilterOrder::Two,
            &one_record,
            success_for(&one_key, &one_record, oracle_bits(&one_key, &one_record)),
        )
        .unwrap();
        let wrong_order_cell = ResponseCellAssessment::from_test(
            one_key,
            identity_for(&one_key, &one_record).unwrap(),
            wrong_order_assessment,
            None,
        );
        let order_error = compare_physical_scaling_candidates(
            &wrong_order_cell,
            &one_record,
            &thirty_seven_cell,
            &thirty_seven_record,
        )
        .unwrap_err();
        assert!(matches!(
            order_error.kind(),
            CrossCellComparisonErrorKind::OracleOrderMismatch {
                key_order: FilterOrder::One,
                oracle_order: 2,
                ..
            }
        ));
        assert_eq!(order_error.first_key(), one_key);
        assert_eq!(order_error.second_key(), thirty_seven_key);

        let swapped_cells = compare_physical_scaling_candidates(
            &thirty_seven_cell,
            &thirty_seven_record,
            &one_cell,
            &one_record,
        )
        .unwrap_err();
        assert!(matches!(
            swapped_cells.kind(),
            CrossCellComparisonErrorKind::KeyMismatch {
                member: ComparisonMember::OneHz,
                field: KeyField::Cutoff,
                ..
            }
        ));
        assert_eq!(swapped_cells.first_key(), thirty_seven_key);
        assert_eq!(swapped_cells.second_key(), one_key);
    }

    #[test]
    fn physical_scaling_rejects_shape_samples_scalar_and_cutoff_mismatches() {
        let one_record = simple_record(RecordScalar::F64, PhysicalCutoff::OneHz, &[0.25]);
        let one_key = key_for_simple_physical(RecordScalar::F64, PhysicalCutoff::OneHz);
        let one_cell = passing_cell(one_key, &one_record);

        let long_record = simple_record(RecordScalar::F64, PhysicalCutoff::ThirtySevenHz, &[0.125, 0.25]);
        let long_key = key_for_simple_physical(RecordScalar::F64, PhysicalCutoff::ThirtySevenHz);
        let long_cell = passing_cell(long_key, &long_record);
        let shape_error = compare_physical_scaling_candidates(
            &one_cell,
            &one_record,
            &long_cell,
            &long_record,
        )
        .unwrap_err();
        assert!(matches!(
            shape_error.kind(),
            CrossCellComparisonErrorKind::PairShapeMismatch { .. }
        ));
        assert_eq!(shape_error.first_key(), one_key);
        assert_eq!(shape_error.second_key(), long_key);

        let changed_record = RepresentedRecord::from_f64(
            37.0,
            vec![0.125],
            vec![0.0, 0.75],
        )
        .unwrap();
        let changed_cell = passing_cell(long_key, &changed_record);
        let sample_error = compare_physical_scaling_candidates(
            &one_cell,
            &one_record,
            &changed_cell,
            &changed_record,
        )
        .unwrap_err();
        assert!(matches!(
            sample_error.kind(),
            CrossCellComparisonErrorKind::SampleMismatch { index: 1, .. }
        ));
        assert_eq!(sample_error.first_key(), one_key);
        assert_eq!(sample_error.second_key(), long_key);

        let f32_record = simple_record(RecordScalar::F32, PhysicalCutoff::ThirtySevenHz, &[0.125]);
        let f32_key = key_for_simple_physical(RecordScalar::F32, PhysicalCutoff::ThirtySevenHz);
        let f32_cell = passing_cell(f32_key, &f32_record);
        let scalar_error = compare_physical_scaling_candidates(
            &one_cell,
            &one_record,
            &f32_cell,
            &f32_record,
        )
        .unwrap_err();
        assert!(matches!(
            scalar_error.kind(),
            CrossCellComparisonErrorKind::KeyMismatch {
                member: ComparisonMember::ThirtySevenHz,
                field: KeyField::Scalar,
                ..
            }
        ));
        assert_eq!(scalar_error.first_key(), one_key);
        assert_eq!(scalar_error.second_key(), f32_key);

        let wrong_cutoff_key = key_for_simple_physical(RecordScalar::F64, PhysicalCutoff::ThirtySevenHz);
        let wrong_cutoff_cell = passing_cell(wrong_cutoff_key, &long_record);
        let cutoff_error = compare_physical_scaling_candidates(
            &wrong_cutoff_cell,
            &long_record,
            &long_cell,
            &long_record,
        )
        .unwrap_err();
        assert!(matches!(
            cutoff_error.kind(),
            CrossCellComparisonErrorKind::KeyMismatch {
                member: ComparisonMember::OneHz,
                field: KeyField::Cutoff,
                ..
            }
        ));
        assert_eq!(cutoff_error.first_key(), wrong_cutoff_key);
        assert_eq!(cutoff_error.second_key(), long_key);
    }
}
