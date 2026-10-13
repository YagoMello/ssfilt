//! Materializes validated candidate results for later numerical scoring.

use core::fmt;

use forward_backward_protocol::{
    CandidateErrorResponse, CaseResult, OutputBits, PassCounters, SuccessResponse, TailEvidence,
};
use rug::Float;

use crate::{RecordScalar, RepresentedRecord};

/// Precision used to preserve exact native candidate values for scoring.
pub(crate) const CANDIDATE_OUTPUT_PRECISION: u32 = 256;

/// A successfully materialized candidate result, structured error, or cell error.
#[derive(Debug)]
pub(crate) enum CandidateOutcome {
    /// Finite native output promoted exactly to [`Float`] at 256-bit precision.
    Success(CandidateSuccess),
    /// Structured candidate failure preserved from the wire result.
    CandidateError(CandidateErrorResponse),
    /// A successful response whose first non-finite output value is preserved by its bits.
    NonFiniteSuccess(NonFiniteSuccess),
}

/// Work diagnostics from the candidate's three passes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CandidateWorkCounters {
    forward: PassCounters,
    tail: PassCounters,
    reverse: PassCounters,
}

impl CandidateWorkCounters {
    #[must_use]
    pub(crate) const fn forward(self) -> PassCounters {
        self.forward
    }

    #[must_use]
    pub(crate) const fn tail(self) -> PassCounters {
        self.tail
    }

    #[must_use]
    pub(crate) const fn reverse(self) -> PassCounters {
        self.reverse
    }
}

/// A finite candidate success ready for high-precision scoring.
#[derive(Debug)]
pub(crate) struct CandidateSuccess {
    output: Vec<Float>,
    work: CandidateWorkCounters,
    tail_evidence: TailEvidence,
}

impl CandidateSuccess {
    #[must_use]
    pub(crate) fn output(&self) -> &[Float] {
        &self.output
    }

    #[must_use]
    pub(crate) const fn work(&self) -> CandidateWorkCounters {
        self.work
    }

    #[must_use]
    pub(crate) const fn tail_evidence(&self) -> &TailEvidence {
        &self.tail_evidence
    }

    /// Moves all materialized data into the scorer without cloning the output.
    pub(crate) fn into_parts(self) -> (Vec<Float>, CandidateWorkCounters, TailEvidence) {
        (self.output, self.work, self.tail_evidence)
    }
}

/// The exact native-width payload for one candidate output value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateValueBits {
    /// An `f32` IEEE-754 payload.
    F32(u32),
    /// An `f64` IEEE-754 payload.
    F64(u64),
}

impl CandidateValueBits {
    fn is_finite(self) -> bool {
        match self {
            Self::F32(bits) => f32::from_bits(bits).is_finite(),
            Self::F64(bits) => f64::from_bits(bits).is_finite(),
        }
    }

    fn promote_finite(self) -> Float {
        match self {
            Self::F32(bits) => {
                let value = f32::from_bits(bits);
                debug_assert!(value.is_finite(), "output was checked before reservation");
                Float::with_val(CANDIDATE_OUTPUT_PRECISION, value)
            }
            Self::F64(bits) => {
                let value = f64::from_bits(bits);
                debug_assert!(value.is_finite(), "output was checked before reservation");
                Float::with_val(CANDIDATE_OUTPUT_PRECISION, value)
            }
        }
    }
}

/// The first non-finite value in an otherwise successful response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NonFiniteSuccess {
    index: usize,
    value_bits: CandidateValueBits,
    work: CandidateWorkCounters,
    tail_evidence: TailEvidence,
}

impl NonFiniteSuccess {
    #[must_use]
    pub(crate) const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub(crate) const fn value_bits(&self) -> CandidateValueBits {
        self.value_bits
    }

    #[must_use]
    pub(crate) const fn work(&self) -> CandidateWorkCounters {
        self.work
    }

    #[must_use]
    pub(crate) const fn tail_evidence(&self) -> &TailEvidence {
        &self.tail_evidence
    }

    /// Moves the exact failure and accompanying evidence into a collector.
    pub(crate) fn into_parts(
        self,
    ) -> (
        usize,
        CandidateValueBits,
        CandidateWorkCounters,
        TailEvidence,
    ) {
        (self.index, self.value_bits, self.work, self.tail_evidence)
    }
}

/// Fatal errors indicating that a validated result cannot be materialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateMaterializationError {
    /// The candidate output width disagrees with the represented input record.
    OutputScalarMismatch {
        expected: RecordScalar,
        actual: RecordScalar,
    },
    /// The candidate output count disagrees with the represented record.
    OutputLengthMismatch { expected: usize, actual: usize },
    /// The single output buffer could not reserve its exact requested capacity.
    CapacityReservationFailed { requested_capacity: usize },
}

impl fmt::Display for CandidateMaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputScalarMismatch { expected, actual } => write!(
                formatter,
                "candidate output uses {actual:?}; represented record uses {expected:?}"
            ),
            Self::OutputLengthMismatch { expected, actual } => write!(
                formatter,
                "candidate output has {actual} values; represented record has {expected} samples"
            ),
            Self::CapacityReservationFailed { requested_capacity } => write!(
                formatter,
                "could not reserve candidate output capacity for {requested_capacity} values"
            ),
        }
    }
}

impl std::error::Error for CandidateMaterializationError {}

/// Promotes a validated candidate response without cloning its output bits.
///
/// Case identity and wire-level response validation belong to the session and
/// protocol layers; this function checks only materialization requirements.
pub(crate) fn materialize_candidate_result(
    result: CaseResult,
    record: &RepresentedRecord,
) -> Result<CandidateOutcome, CandidateMaterializationError> {
    match result {
        CaseResult::CandidateError(response) => Ok(CandidateOutcome::CandidateError(response)),
        CaseResult::Success(response) => materialize_success(response, record),
    }
}

fn materialize_success(
    response: SuccessResponse,
    record: &RepresentedRecord,
) -> Result<CandidateOutcome, CandidateMaterializationError> {
    let expected_scalar = record.scalar();
    let expected_len = record.samples().len();
    let (actual_scalar, actual_len) = match &response.output_bits {
        OutputBits::F32(values) => (RecordScalar::F32, values.len()),
        OutputBits::F64(values) => (RecordScalar::F64, values.len()),
    };
    if actual_scalar != expected_scalar {
        return Err(CandidateMaterializationError::OutputScalarMismatch {
            expected: expected_scalar,
            actual: actual_scalar,
        });
    }
    if actual_len != expected_len {
        return Err(CandidateMaterializationError::OutputLengthMismatch {
            expected: expected_len,
            actual: actual_len,
        });
    }

    // Scan borrowed bits before reserving: a collectable non-finite cell must
    // not be masked by a later failure to allocate its promoted output buffer.
    let first_non_finite = first_non_finite_output(&response.output_bits);

    let SuccessResponse {
        output_bits,
        tail_evidence,
        forward,
        tail,
        reverse,
        ..
    } = response;
    let work = CandidateWorkCounters {
        forward,
        tail,
        reverse,
    };

    if let Some((index, value_bits)) = first_non_finite {
        return Ok(CandidateOutcome::NonFiniteSuccess(NonFiniteSuccess {
            index,
            value_bits,
            work,
            tail_evidence,
        }));
    }

    match output_bits {
        OutputBits::F32(values) => materialize_values(
            values.into_iter().map(CandidateValueBits::F32),
            expected_len,
            work,
            tail_evidence,
        ),
        OutputBits::F64(values) => materialize_values(
            values.into_iter().map(CandidateValueBits::F64),
            expected_len,
            work,
            tail_evidence,
        ),
    }
}

fn first_non_finite_output(output_bits: &OutputBits) -> Option<(usize, CandidateValueBits)> {
    match output_bits {
        OutputBits::F32(values) => values
            .iter()
            .copied()
            .enumerate()
            .find_map(|(index, bits)| {
                let value_bits = CandidateValueBits::F32(bits);
                (!value_bits.is_finite()).then_some((index, value_bits))
            }),
        OutputBits::F64(values) => values
            .iter()
            .copied()
            .enumerate()
            .find_map(|(index, bits)| {
                let value_bits = CandidateValueBits::F64(bits);
                (!value_bits.is_finite()).then_some((index, value_bits))
            }),
    }
}

fn materialize_values(
    values: impl Iterator<Item = CandidateValueBits>,
    expected_len: usize,
    work: CandidateWorkCounters,
    tail_evidence: TailEvidence,
) -> Result<CandidateOutcome, CandidateMaterializationError> {
    let mut output = Vec::new();
    output.try_reserve_exact(expected_len).map_err(|_| {
        CandidateMaterializationError::CapacityReservationFailed {
            requested_capacity: expected_len,
        }
    })?;

    for value_bits in values {
        output.push(value_bits.promote_finite());
    }

    Ok(CandidateOutcome::Success(CandidateSuccess {
        output,
        work,
        tail_evidence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use forward_backward_protocol::{
        CandidateErrorCategory, CandidateErrorResponse, CaseIdentity, Pass, SourceCategory,
    };

    fn record_f32(sample_count: usize) -> RepresentedRecord {
        RepresentedRecord::from_f32(1.0, vec![0.125; sample_count - 1], vec![0.0; sample_count])
            .unwrap()
    }

    fn record_f64(sample_count: usize) -> RepresentedRecord {
        RepresentedRecord::from_f64(1.0, vec![0.125; sample_count - 1], vec![0.0; sample_count])
            .unwrap()
    }

    fn work() -> CandidateWorkCounters {
        CandidateWorkCounters {
            forward: PassCounters {
                accepted_steps: 11,
                rejected_steps: 2,
                derivative_evaluations: 77,
            },
            tail: PassCounters {
                accepted_steps: 13,
                rejected_steps: 3,
                derivative_evaluations: 91,
            },
            reverse: PassCounters {
                accepted_steps: 17,
                rejected_steps: 5,
                derivative_evaluations: 119,
            },
        }
    }

    fn tail_evidence(output_bits: &OutputBits) -> TailEvidence {
        match output_bits {
            OutputBits::F32(values) => TailEvidence::NumericalF32 {
                final_residual_bits: 0x3d00_0000,
                public_normalized_extension_bits: 0x3f80_0000,
                analytic_output_bits: vec![0; values.len()],
            },
            OutputBits::F64(values) => TailEvidence::NumericalF64 {
                final_residual_bits: 0x3fa0_0000_0000_0000,
                public_normalized_extension_bits: 0x3ff0_0000_0000_0000,
                analytic_output_bits: vec![0; values.len()],
            },
        }
    }

    fn success(output_bits: OutputBits) -> SuccessResponse {
        let tail_evidence = tail_evidence(&output_bits);
        SuccessResponse {
            identity: CaseIdentity {
                case_id: 7,
                record_digest: [0x5a; 32],
            },
            output_bits,
            tail_evidence,
            forward: work().forward(),
            tail: work().tail(),
            reverse: work().reverse(),
        }
    }

    #[test]
    fn promotes_f32_finite_values_exactly_including_signed_zero_and_subnormal() {
        let bits = [
            0x8000_0000,
            0x0000_0000,
            0x0000_0001,
            0x3dcc_cccd,
            0xc148_0000,
        ];
        let output_bits = OutputBits::F32(bits.to_vec());
        let expected_tail_evidence = tail_evidence(&output_bits);
        let outcome = materialize_candidate_result(
            CaseResult::Success(success(output_bits)),
            &record_f32(bits.len()),
        )
        .unwrap();

        let CandidateOutcome::Success(candidate) = outcome else {
            panic!("expected finite candidate success");
        };
        assert_eq!(
            candidate
                .output()
                .iter()
                .map(Float::to_f32)
                .map(f32::to_bits)
                .collect::<Vec<_>>(),
            bits
        );
        assert!(candidate.output().iter().all(|value| value.prec() == 256));
        assert_eq!(candidate.work(), work());
        assert_eq!(candidate.tail_evidence(), &expected_tail_evidence);
    }

    #[test]
    fn promotes_f64_finite_values_exactly_including_signed_zero_and_subnormal() {
        let bits = [
            0x8000_0000_0000_0000,
            0x0000_0000_0000_0000,
            0x0000_0000_0000_0001,
            0x3fb9_9999_9999_999a,
            0xc029_0000_0000_0000,
        ];
        let output_bits = OutputBits::F64(bits.to_vec());
        let expected_tail_evidence = tail_evidence(&output_bits);
        let outcome = materialize_candidate_result(
            CaseResult::Success(success(output_bits)),
            &record_f64(bits.len()),
        )
        .unwrap();

        let CandidateOutcome::Success(candidate) = outcome else {
            panic!("expected finite candidate success");
        };
        assert_eq!(
            candidate
                .output()
                .iter()
                .map(Float::to_f64)
                .map(f64::to_bits)
                .collect::<Vec<_>>(),
            bits
        );
        assert!(candidate.output().iter().all(|value| value.prec() == 256));
        assert_eq!(candidate.work(), work());
        assert_eq!(candidate.tail_evidence(), &expected_tail_evidence);
    }

    #[test]
    fn classifies_the_first_f32_nan_and_preserves_its_payload_bits() {
        let payload = 0xffc1_2345;
        let output_bits = OutputBits::F32(vec![0x3f80_0000, payload, 0x7f80_0000, 0x7fc0_0001]);
        let expected_tail_evidence = tail_evidence(&output_bits);
        let outcome =
            materialize_candidate_result(CaseResult::Success(success(output_bits)), &record_f32(4))
                .unwrap();

        let CandidateOutcome::NonFiniteSuccess(non_finite) = outcome else {
            panic!("expected a non-finite candidate success");
        };
        assert_eq!(non_finite.index(), 1);
        assert_eq!(non_finite.value_bits(), CandidateValueBits::F32(payload));
        assert_eq!(non_finite.work(), work());
        assert_eq!(non_finite.tail_evidence(), &expected_tail_evidence);
    }

    #[test]
    fn classifies_the_first_f64_nan_and_preserves_its_payload_bits() {
        let payload = 0xfff8_0000_0001_2345;
        let output_bits = OutputBits::F64(vec![
            0x3ff0_0000_0000_0000,
            payload,
            0x7ff0_0000_0000_0000,
            0x7ff8_0000_0000_0001,
        ]);
        let expected_tail_evidence = tail_evidence(&output_bits);
        let outcome =
            materialize_candidate_result(CaseResult::Success(success(output_bits)), &record_f64(4))
                .unwrap();

        let CandidateOutcome::NonFiniteSuccess(non_finite) = outcome else {
            panic!("expected a non-finite candidate success");
        };
        assert_eq!(non_finite.index(), 1);
        assert_eq!(non_finite.value_bits(), CandidateValueBits::F64(payload));
        assert_eq!(non_finite.work(), work());
        assert_eq!(non_finite.tail_evidence(), &expected_tail_evidence);
    }

    #[test]
    fn classifies_f32_positive_and_negative_infinity_at_the_first_non_finite_index() {
        for (prefix, infinity_bits) in [(vec![0x3f80_0000], 0xff80_0000), (Vec::new(), 0x7f80_0000)]
        {
            let index = prefix.len();
            let mut bits = prefix;
            bits.push(infinity_bits);
            bits.push(if infinity_bits == 0xff80_0000 {
                0x7f80_0000
            } else {
                0xff80_0000
            });
            let outcome = materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F32(bits))),
                &record_f32(index + 2),
            )
            .unwrap();

            let CandidateOutcome::NonFiniteSuccess(non_finite) = outcome else {
                panic!("expected a non-finite candidate success");
            };
            assert_eq!(non_finite.index(), index);
            assert_eq!(
                non_finite.value_bits(),
                CandidateValueBits::F32(infinity_bits)
            );
        }
    }

    #[test]
    fn classifies_f64_positive_and_negative_infinity_at_the_first_non_finite_index() {
        for (prefix, infinity_bits) in [
            (vec![0x3ff0_0000_0000_0000], 0xfff0_0000_0000_0000),
            (Vec::new(), 0x7ff0_0000_0000_0000),
        ] {
            let index = prefix.len();
            let mut bits = prefix;
            bits.push(infinity_bits);
            bits.push(if infinity_bits == 0xfff0_0000_0000_0000 {
                0x7ff0_0000_0000_0000
            } else {
                0xfff0_0000_0000_0000
            });
            let outcome = materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F64(bits))),
                &record_f64(index + 2),
            )
            .unwrap();

            let CandidateOutcome::NonFiniteSuccess(non_finite) = outcome else {
                panic!("expected a non-finite candidate success");
            };
            assert_eq!(non_finite.index(), index);
            assert_eq!(
                non_finite.value_bits(),
                CandidateValueBits::F64(infinity_bits)
            );
        }
    }

    #[test]
    fn preserves_structured_candidate_errors_unchanged() {
        let response = CandidateErrorResponse {
            identity: CaseIdentity {
                case_id: 19,
                record_digest: [0xa5; 32],
            },
            category: CandidateErrorCategory::Integration,
            pass: Some(Pass::Reverse),
            interval: Some(3),
            source_category: Some(SourceCategory::StepBudgetExceeded),
        };
        let outcome = materialize_candidate_result(
            CaseResult::CandidateError(response.clone()),
            &record_f32(2),
        )
        .unwrap();

        assert!(matches!(outcome, CandidateOutcome::CandidateError(actual) if actual == response));
    }

    #[test]
    fn reports_output_scalar_mismatch_for_both_widths() {
        assert_eq!(
            materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F64(vec![0]))),
                &record_f32(1),
            )
            .unwrap_err(),
            CandidateMaterializationError::OutputScalarMismatch {
                expected: RecordScalar::F32,
                actual: RecordScalar::F64,
            }
        );
        assert_eq!(
            materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F32(vec![0]))),
                &record_f64(1),
            )
            .unwrap_err(),
            CandidateMaterializationError::OutputScalarMismatch {
                expected: RecordScalar::F64,
                actual: RecordScalar::F32,
            }
        );
    }

    #[test]
    fn reports_output_length_mismatch_for_both_widths() {
        assert_eq!(
            materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F32(vec![0]))),
                &record_f32(2),
            )
            .unwrap_err(),
            CandidateMaterializationError::OutputLengthMismatch {
                expected: 2,
                actual: 1,
            }
        );
        assert_eq!(
            materialize_candidate_result(
                CaseResult::Success(success(OutputBits::F64(vec![0]))),
                &record_f64(2),
            )
            .unwrap_err(),
            CandidateMaterializationError::OutputLengthMismatch {
                expected: 2,
                actual: 1,
            }
        );
    }
}
