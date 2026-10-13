//! Certified single-record candidate scoring and frozen RMS/peak gates.

use core::fmt;

use forward_backward_protocol::{CandidateErrorResponse, TailEvidence};
use rug::Float;

use crate::assessment::{
    AssessmentError, CertifiedOracle, ErrorMetrics, HIGH_ORACLE_PRECISION, certify_oracle,
    weighted_error,
};
use crate::candidate::{CandidateOutcome, CandidateWorkCounters, NonFiniteSuccess};
use crate::{RecordScalar, RepresentedRecord};

const F64_RMS_LIMIT: (u32, u32) = (5, 100_000);
const F64_PEAK_LIMIT: (u32, u32) = (2, 10_000);
const F32_RMS_LIMIT: (u32, u32) = (2, 1_000);
const F32_PEAK_LIMIT: (u32, u32) = (8, 1_000);

/// Compact integrity diagnostics retained after dropping the oracle outputs.
#[derive(Clone, Debug)]
pub(crate) struct OracleDiagnostics {
    order: usize,
    convergence: ErrorMetrics,
    low_imaginary_ratio: Float,
    high_imaginary_ratio: Float,
}

impl OracleDiagnostics {
    #[must_use]
    pub(crate) const fn order(&self) -> usize {
        self.order
    }

    #[must_use]
    pub(crate) const fn convergence(&self) -> &ErrorMetrics {
        &self.convergence
    }

    #[must_use]
    pub(crate) const fn low_imaginary_ratio(&self) -> &Float {
        &self.low_imaginary_ratio
    }

    #[must_use]
    pub(crate) const fn high_imaginary_ratio(&self) -> &Float {
        &self.high_imaginary_ratio
    }
}

/// One frozen normalized-error threshold and whether its inclusive gate passed.
#[derive(Clone, Debug)]
pub(crate) struct MetricGate {
    limit: Float,
    passed: bool,
}

impl MetricGate {
    #[must_use]
    pub(crate) const fn limit(&self) -> &Float {
        &self.limit
    }

    #[must_use]
    pub(crate) const fn passed(&self) -> bool {
        self.passed
    }
}

/// Frozen normalized RMS and endpoint-inclusive peak acceptance gates.
#[derive(Clone, Debug)]
pub(crate) struct CandidateGates {
    rms: MetricGate,
    peak: MetricGate,
}

impl CandidateGates {
    #[must_use]
    pub(crate) const fn rms(&self) -> &MetricGate {
        &self.rms
    }

    #[must_use]
    pub(crate) const fn peak(&self) -> &MetricGate {
        &self.peak
    }

    /// Returns true only when both whole-record numerical gates passed.
    #[must_use]
    pub(crate) const fn passed(&self) -> bool {
        self.rms.passed && self.peak.passed
    }
}

/// Candidate result for one certified represented record.
#[derive(Debug)]
pub(crate) enum CandidateDisposition {
    /// A finite candidate output was measured against the canonical oracle.
    Scored {
        metrics: ErrorMetrics,
        gates: CandidateGates,
        work: CandidateWorkCounters,
        tail_evidence: TailEvidence,
    },
    /// Structured failure reported by the candidate process.
    CandidateError(CandidateErrorResponse),
    /// Successful response containing a non-finite native output value.
    NonFiniteSuccess(NonFiniteSuccess),
}

/// Whole-record outcome plus compact diagnostics from its certified oracle.
#[derive(Debug)]
pub(crate) struct WholeRecordAssessment {
    oracle: OracleDiagnostics,
    candidate: CandidateDisposition,
}

impl WholeRecordAssessment {
    #[must_use]
    pub(crate) const fn oracle(&self) -> &OracleDiagnostics {
        &self.oracle
    }

    #[must_use]
    pub(crate) const fn candidate(&self) -> &CandidateDisposition {
        &self.candidate
    }
}

/// Fatal oracle-integrity or candidate-metric failure for one record.
#[derive(Debug)]
pub(crate) enum WholeRecordFatalError {
    /// The independent oracle failed certification before candidate scoring.
    OracleIntegrity(AssessmentError),
    /// A finite candidate series could not be assessed against the reference.
    CandidateMetric(AssessmentError),
}

impl fmt::Display for WholeRecordFatalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OracleIntegrity(error) => write!(formatter, "oracle integrity failed: {error}"),
            Self::CandidateMetric(error) => write!(formatter, "candidate metric failed: {error}"),
        }
    }
}

impl std::error::Error for WholeRecordFatalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OracleIntegrity(error) | Self::CandidateMetric(error) => Some(error),
        }
    }
}

/// Certifies the oracle, then scores one finite candidate output if available.
///
/// Candidate errors and non-finite successes remain collectable cell outcomes,
/// but each still receives fresh oracle-integrity diagnostics. The canonical
/// reference and candidate output vectors are dropped before returning.
pub(crate) fn score_whole_record(
    order: usize,
    record: &RepresentedRecord,
    candidate: CandidateOutcome,
) -> Result<WholeRecordAssessment, WholeRecordFatalError> {
    // Certification deliberately precedes the candidate-result match: oracle
    // integrity is an independent gate, even for collectable candidate errors.
    let certified =
        certify_oracle(order, record).map_err(WholeRecordFatalError::OracleIntegrity)?;
    let oracle = oracle_diagnostics(&certified);

    let candidate = match candidate {
        CandidateOutcome::Success(success) => {
            let (output, work, tail_evidence) = success.into_parts();
            let metrics = weighted_error(
                record,
                &output,
                certified.reference().values(),
                HIGH_ORACLE_PRECISION,
            )
            .map_err(WholeRecordFatalError::CandidateMetric)?;
            drop(output);
            let gates = candidate_gates(record.scalar(), &metrics);
            CandidateDisposition::Scored {
                metrics,
                gates,
                work,
                tail_evidence,
            }
        }
        CandidateOutcome::CandidateError(response) => {
            CandidateDisposition::CandidateError(response)
        }
        CandidateOutcome::NonFiniteSuccess(response) => {
            CandidateDisposition::NonFiniteSuccess(response)
        }
    };

    Ok(WholeRecordAssessment { oracle, candidate })
}

fn oracle_diagnostics(certified: &CertifiedOracle) -> OracleDiagnostics {
    let assessment = certified.assessment();
    OracleDiagnostics {
        order: assessment.order(),
        convergence: assessment.convergence().clone(),
        low_imaginary_ratio: assessment.low_imaginary_ratio().clone(),
        high_imaginary_ratio: assessment.high_imaginary_ratio().clone(),
    }
}

fn candidate_gates(scalar: RecordScalar, metrics: &ErrorMetrics) -> CandidateGates {
    let (rms_limit, peak_limit) = gate_limits(scalar);
    CandidateGates {
        rms: metric_gate(metrics.normalized_rms(), rms_limit),
        peak: metric_gate(metrics.normalized_peak(), peak_limit),
    }
}

fn gate_limits(scalar: RecordScalar) -> (Float, Float) {
    let (rms, peak) = match scalar {
        RecordScalar::F64 => (F64_RMS_LIMIT, F64_PEAK_LIMIT),
        RecordScalar::F32 => (F32_RMS_LIMIT, F32_PEAK_LIMIT),
    };
    (decimal_rational(rms), decimal_rational(peak))
}

fn decimal_rational((numerator, denominator): (u32, u32)) -> Float {
    let mut value = Float::with_val(HIGH_ORACLE_PRECISION, numerator);
    value /= denominator;
    value
}

fn metric_gate(measured: &Float, limit: Float) -> MetricGate {
    MetricGate {
        passed: measured <= &limit,
        limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forward_backward_protocol::{
        CandidateErrorCategory, CaseIdentity, OutputBits, Pass, PassCounters, SourceCategory,
        SuccessResponse,
    };

    use crate::candidate::{CandidateValueBits, materialize_candidate_result};
    use crate::{RecordScalar, certify_oracle};

    const IDENTITY: CaseIdentity = CaseIdentity {
        case_id: 31,
        record_digest: [0x3c; 32],
    };

    fn f32_record() -> RepresentedRecord {
        RepresentedRecord::from_f32(1.0, vec![], vec![0.5]).unwrap()
    }

    fn f64_record() -> RepresentedRecord {
        RepresentedRecord::from_f64(1.0, vec![], vec![0.5]).unwrap()
    }

    fn work() -> (PassCounters, PassCounters, PassCounters) {
        (
            PassCounters {
                accepted_steps: 7,
                rejected_steps: 1,
                derivative_evaluations: 49,
            },
            PassCounters {
                accepted_steps: 11,
                rejected_steps: 2,
                derivative_evaluations: 77,
            },
            PassCounters {
                accepted_steps: 13,
                rejected_steps: 3,
                derivative_evaluations: 91,
            },
        )
    }

    fn success_response(output_bits: OutputBits) -> SuccessResponse {
        let tail_evidence = match &output_bits {
            OutputBits::F32(values) => TailEvidence::NumericalF32 {
                final_residual_bits: 0x3d00_0000,
                public_normalized_extension_bits: 0x3f80_0000,
                analytic_output_bits: vec![0x3e00_0000; values.len()],
            },
            OutputBits::F64(values) => TailEvidence::NumericalF64 {
                final_residual_bits: 0x3fa0_0000_0000_0000,
                public_normalized_extension_bits: 0x3ff0_0000_0000_0000,
                analytic_output_bits: vec![0x3fe0_0000_0000_0000; values.len()],
            },
        };
        let (forward, tail, reverse) = work();
        SuccessResponse {
            identity: IDENTITY,
            output_bits,
            tail_evidence,
            forward,
            tail,
            reverse,
        }
    }

    fn materialized(output_bits: OutputBits, record: &RepresentedRecord) -> CandidateOutcome {
        materialize_candidate_result(
            forward_backward_protocol::CaseResult::Success(success_response(output_bits)),
            record,
        )
        .unwrap()
    }

    fn assert_matches_fresh_diagnostics(
        actual: &OracleDiagnostics,
        order: usize,
        record: &RepresentedRecord,
    ) {
        let expected = certify_oracle(order, record).unwrap();
        let expected = expected.assessment();
        assert_eq!(actual.order(), expected.order());
        assert_eq!(
            actual.convergence().absolute_rms(),
            expected.convergence().absolute_rms()
        );
        assert_eq!(
            actual.convergence().normalized_rms(),
            expected.convergence().normalized_rms()
        );
        assert_eq!(
            actual.convergence().absolute_peak(),
            expected.convergence().absolute_peak()
        );
        assert_eq!(
            actual.convergence().normalized_peak(),
            expected.convergence().normalized_peak()
        );
        assert_eq!(actual.low_imaginary_ratio(), expected.low_imaginary_ratio());
        assert_eq!(
            actual.high_imaginary_ratio(),
            expected.high_imaginary_ratio()
        );
    }

    fn assert_threshold_equality_and_next_up(limit: Float) {
        assert_eq!(limit.prec(), HIGH_ORACLE_PRECISION);
        let at_limit = metric_gate(&limit, limit.clone());
        assert!(at_limit.passed());
        assert_eq!(at_limit.limit(), &limit);

        let mut next = limit.clone();
        next.next_up();
        assert_eq!(next.prec(), HIGH_ORACLE_PRECISION);
        assert!(next > limit);
        assert!(!metric_gate(&next, limit).passed());
    }

    #[test]
    fn each_frozen_threshold_accepts_equality_and_rejects_one_256_bit_step_above() {
        let frozen_decimal_limits = [
            (RecordScalar::F64, "0.00005", "0.0002"),
            (RecordScalar::F32, "0.002", "0.008"),
        ];
        for (scalar, rms_decimal, peak_decimal) in frozen_decimal_limits {
            let (rms, peak) = gate_limits(scalar);
            let expected_rms =
                Float::with_val(HIGH_ORACLE_PRECISION, Float::parse(rms_decimal).unwrap());
            let expected_peak =
                Float::with_val(HIGH_ORACLE_PRECISION, Float::parse(peak_decimal).unwrap());
            assert_eq!(rms, expected_rms, "{scalar:?} normalized RMS threshold");
            assert_eq!(peak, expected_peak, "{scalar:?} normalized peak threshold");
            assert_threshold_equality_and_next_up(rms);
            assert_threshold_equality_and_next_up(peak);
        }
    }

    #[test]
    fn zero_error_f32_and_f64_scores_pass_and_retain_diagnostics_and_work() {
        for (record, output_bits) in [
            (f32_record(), OutputBits::F32(vec![0.5_f32.to_bits()])),
            (f64_record(), OutputBits::F64(vec![0.5_f64.to_bits()])),
        ] {
            let result =
                score_whole_record(1, &record, materialized(output_bits, &record)).unwrap();
            assert_matches_fresh_diagnostics(result.oracle(), 1, &record);
            assert_eq!(result.oracle().order(), 1);
            match result.candidate() {
                CandidateDisposition::Scored {
                    metrics,
                    gates,
                    work: actual_work,
                    tail_evidence,
                } => {
                    let expected_work = work();
                    assert_eq!(metrics.absolute_rms(), &Float::with_val(256, 0));
                    assert_eq!(metrics.normalized_rms(), &Float::with_val(256, 0));
                    assert_eq!(metrics.absolute_peak(), &Float::with_val(256, 0));
                    assert_eq!(metrics.normalized_peak(), &Float::with_val(256, 0));
                    assert!(gates.passed());
                    assert_eq!(actual_work.forward(), expected_work.0);
                    assert_eq!(actual_work.tail(), expected_work.1);
                    assert_eq!(actual_work.reverse(), expected_work.2);
                    match record.scalar() {
                        RecordScalar::F32 => assert!(matches!(
                            tail_evidence,
                            TailEvidence::NumericalF32 {
                                final_residual_bits: 0x3d00_0000,
                                public_normalized_extension_bits: 0x3f80_0000,
                                analytic_output_bits,
                            } if analytic_output_bits == &[0x3e00_0000]
                        )),
                        RecordScalar::F64 => assert!(matches!(
                            tail_evidence,
                            TailEvidence::NumericalF64 {
                                final_residual_bits: 0x3fa0_0000_0000_0000,
                                public_normalized_extension_bits: 0x3ff0_0000_0000_0000,
                                analytic_output_bits,
                            } if analytic_output_bits == &[0x3fe0_0000_0000_0000]
                        )),
                    }
                }
                other => panic!("expected a scored finite result, got {other:?}"),
            }
        }
    }

    #[test]
    fn large_finite_error_is_a_scored_gate_failure_not_a_fatal_error() {
        let record = f64_record();
        let result = score_whole_record(
            1,
            &record,
            materialized(OutputBits::F64(vec![2.0_f64.to_bits()]), &record),
        )
        .unwrap();
        let CandidateDisposition::Scored { metrics, gates, .. } = result.candidate() else {
            panic!("expected a scored finite result");
        };
        assert!(*metrics.normalized_rms() > Float::with_val(256, 0));
        assert!(*metrics.normalized_peak() > Float::with_val(256, 0));
        assert!(!gates.passed());
        assert!(!gates.rms().passed());
        assert!(!gates.peak().passed());
    }

    #[test]
    fn candidate_error_and_non_finite_remain_collectable_with_oracle_diagnostics() {
        let record = RepresentedRecord::from_f64(1.0, vec![0.125], vec![0.5, 0.5]).unwrap();
        let candidate_error = forward_backward_protocol::CandidateErrorResponse {
            identity: IDENTITY,
            category: CandidateErrorCategory::Integration,
            pass: Some(Pass::Reverse),
            interval: Some(0),
            source_category: Some(SourceCategory::StepBudgetExceeded),
        };
        let result = score_whole_record(
            1,
            &record,
            CandidateOutcome::CandidateError(candidate_error.clone()),
        )
        .unwrap();
        assert_matches_fresh_diagnostics(result.oracle(), 1, &record);
        assert!(matches!(
            result.candidate(),
            CandidateDisposition::CandidateError(actual) if actual == &candidate_error
        ));

        let non_finite_outcome = materialized(
            OutputBits::F64(vec![0x7ff8_0000_0001_2345, 0x3fe0_0000_0000_0000]),
            &record,
        );
        let CandidateOutcome::NonFiniteSuccess(expected_non_finite) = &non_finite_outcome else {
            panic!("expected materialized non-finite success");
        };
        let expected_non_finite = expected_non_finite.clone();
        let result = score_whole_record(1, &record, non_finite_outcome).unwrap();
        assert_matches_fresh_diagnostics(result.oracle(), 1, &record);
        assert!(matches!(
            result.candidate(),
            CandidateDisposition::NonFiniteSuccess(actual) if actual == &expected_non_finite
        ));
        assert_eq!(
            expected_non_finite.value_bits(),
            CandidateValueBits::F64(0x7ff8_0000_0001_2345)
        );
    }

    #[test]
    fn unsupported_order_is_an_oracle_integrity_fatal_before_candidate_branching() {
        let record = f64_record();
        let response = forward_backward_protocol::CandidateErrorResponse {
            identity: IDENTITY,
            category: CandidateErrorCategory::Other,
            pass: None,
            interval: None,
            source_category: None,
        };
        let error =
            score_whole_record(3, &record, CandidateOutcome::CandidateError(response)).unwrap_err();
        assert!(matches!(
            error,
            WholeRecordFatalError::OracleIntegrity(error)
                if matches!(error.kind(), crate::AssessmentErrorKind::UnsupportedOrder { order: 3 })
        ));
    }
}
