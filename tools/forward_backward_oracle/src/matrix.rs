//! Private single-cell request construction, execution, and scoring.

use core::fmt;
use std::error::Error as StdError;

use forward_backward_protocol::{CaseIdentity, CaseRequest, FilterOrder, RecordBits};

use crate::candidate::{CandidateMaterializationError, materialize_candidate_result};
use crate::identity::{IdentityError, ResponseCellKey, identity_for};
use crate::score::{WholeRecordAssessment, WholeRecordFatalError, score_whole_record};
use crate::session::{CandidateSession, SessionError};
use crate::{RecordScalar, RepresentedRecord};

/// The response's immutable request provenance and its whole-record outcome.
///
/// Production values can only be made after the matching request has returned
/// and its result has been scored. Keeping the key and identity beside the
/// assessment prevents callers from relabeling a candidate during comparison.
#[derive(Debug)]
pub(crate) struct ResponseCellAssessment {
    key: ResponseCellKey,
    identity: CaseIdentity,
    assessment: WholeRecordAssessment,
    #[cfg(test)]
    output_override: Option<Vec<rug::Float>>,
}

impl ResponseCellAssessment {
    fn from_executed(
        key: ResponseCellKey,
        identity: CaseIdentity,
        assessment: WholeRecordAssessment,
    ) -> Self {
        Self {
            key,
            identity,
            assessment,
            #[cfg(test)]
            output_override: None,
        }
    }

    #[must_use]
    pub(crate) const fn key(&self) -> ResponseCellKey {
        self.key
    }

    #[must_use]
    pub(crate) const fn identity(&self) -> CaseIdentity {
        self.identity
    }

    #[must_use]
    pub(crate) const fn assessment(&self) -> &WholeRecordAssessment {
        &self.assessment
    }

    /// Return the exact promoted output retained by scoring, if finite.
    #[must_use]
    pub(crate) fn scored_output(&self) -> Option<&[rug::Float]> {
        #[cfg(test)]
        if let Some(output) = &self.output_override {
            return Some(output);
        }
        self.assessment.scored_output()
    }

    /// A narrowly scoped seam for comparison tests that need exact 256-bit
    /// boundary values while retaining a real, locally certified assessment.
    #[cfg(test)]
    pub(crate) fn from_test(
        key: ResponseCellKey,
        identity: CaseIdentity,
        assessment: WholeRecordAssessment,
        output_override: Option<Vec<rug::Float>>,
    ) -> Self {
        Self {
            key,
            identity,
            assessment,
            output_override,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_test_output_override(&mut self, output: Vec<rug::Float>) {
        self.output_override = Some(output);
    }
}

/// Which request vector could not reserve its exact required capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequestBuffer {
    Intervals,
    Samples,
}

/// Fatal identity, encoding, or allocation error while building a request.
#[derive(Debug)]
pub(crate) enum RequestBuildError {
    Identity(IdentityError),
    Encoding(IdentityError),
    Allocation {
        buffer: RequestBuffer,
        requested_capacity: usize,
    },
}

impl fmt::Display for RequestBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => write!(formatter, "response-cell identity failed: {error}"),
            Self::Encoding(error) => write!(formatter, "request bit encoding failed: {error}"),
            Self::Allocation {
                buffer,
                requested_capacity,
            } => write!(
                formatter,
                "could not reserve {requested_capacity} values for the {buffer:?} request buffer"
            ),
        }
    }
}

impl StdError for RequestBuildError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Allocation { .. } => None,
        }
    }
}

fn classify_identity_error(error: IdentityError) -> RequestBuildError {
    match error {
        IdentityError::CountTooLarge { .. } | IdentityError::NonExactF32 { .. } => {
            RequestBuildError::Encoding(error)
        }
        IdentityError::ScalarMismatch { .. }
        | IdentityError::CutoffMismatch { .. }
        | IdentityError::ToneOrderMismatch { .. } => RequestBuildError::Identity(error),
    }
}

/// Fatal error in the private single-cell execution path.
#[derive(Debug)]
pub(crate) enum CellExecutionError {
    Request(RequestBuildError),
    Session(SessionError),
    Materialization(CandidateMaterializationError),
    Scoring(WholeRecordFatalError),
}

impl fmt::Display for CellExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => {
                write!(formatter, "could not build response-cell request: {error}")
            }
            Self::Session(error) => write!(formatter, "candidate session failed: {error}"),
            Self::Materialization(error) => {
                write!(formatter, "could not materialize candidate result: {error}")
            }
            Self::Scoring(error) => write!(formatter, "could not score response cell: {error}"),
        }
    }
}

impl StdError for CellExecutionError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::Materialization(error) => Some(error),
            Self::Scoring(error) => Some(error),
        }
    }
}

impl From<RequestBuildError> for CellExecutionError {
    fn from(error: RequestBuildError) -> Self {
        Self::Request(error)
    }
}

impl From<CandidateMaterializationError> for CellExecutionError {
    fn from(error: CandidateMaterializationError) -> Self {
        Self::Materialization(error)
    }
}

impl From<WholeRecordFatalError> for CellExecutionError {
    fn from(error: WholeRecordFatalError) -> Self {
        Self::Scoring(error)
    }
}

/// Execute one response cell using an already-running shard session.
///
/// Identity validation happens before any request vector is allocated. The
/// request and its exact input buffers are dropped as soon as a validated
/// response arrives, before materialization or multiprecision scoring.
pub(crate) fn execute_response_cell(
    session: &mut CandidateSession,
    key: &ResponseCellKey,
    record: &RepresentedRecord,
) -> Result<ResponseCellAssessment, CellExecutionError> {
    let request = build_case_request(key, record)?;
    let response = session
        .round_trip(&request)
        .map_err(CellExecutionError::Session)?;
    let identity = request.identity;
    drop(request);
    let assessment = score_response(key.filter_order(), record, response)?;
    Ok(ResponseCellAssessment::from_executed(
        *key, identity, assessment,
    ))
}

fn build_case_request(
    key: &ResponseCellKey,
    record: &RepresentedRecord,
) -> Result<CaseRequest, RequestBuildError> {
    // Keep this first: it validates identity and exact f32 representation
    // before any request-sized allocation is attempted.
    let identity = identity_for(key, record).map_err(classify_identity_error)?;
    let encoded_record = encode_record(key.scalar(), record)?;

    Ok(CaseRequest {
        identity,
        order: key.filter_order(),
        record: encoded_record,
    })
}

fn encode_record(
    scalar: RecordScalar,
    record: &RepresentedRecord,
) -> Result<RecordBits, RequestBuildError> {
    match scalar {
        RecordScalar::F32 => {
            let interval_count = record.intervals_seconds().len();
            let sample_count = record.samples().len();
            let mut interval_bits = Vec::new();
            interval_bits
                .try_reserve_exact(interval_count)
                .map_err(|_| RequestBuildError::Allocation {
                    buffer: RequestBuffer::Intervals,
                    requested_capacity: interval_count,
                })?;
            let mut sample_bits = Vec::new();
            sample_bits.try_reserve_exact(sample_count).map_err(|_| {
                RequestBuildError::Allocation {
                    buffer: RequestBuffer::Samples,
                    requested_capacity: sample_count,
                }
            })?;

            for &interval in record.intervals_seconds() {
                interval_bits.push((interval as f32).to_bits());
            }
            for &sample in record.samples() {
                sample_bits.push((sample as f32).to_bits());
            }

            Ok(RecordBits::F32 {
                cutoff_bits: (record.cutoff_hz() as f32).to_bits(),
                interval_bits,
                sample_bits,
            })
        }
        RecordScalar::F64 => {
            let interval_count = record.intervals_seconds().len();
            let sample_count = record.samples().len();
            let mut interval_bits = Vec::new();
            interval_bits
                .try_reserve_exact(interval_count)
                .map_err(|_| RequestBuildError::Allocation {
                    buffer: RequestBuffer::Intervals,
                    requested_capacity: interval_count,
                })?;
            let mut sample_bits = Vec::new();
            sample_bits.try_reserve_exact(sample_count).map_err(|_| {
                RequestBuildError::Allocation {
                    buffer: RequestBuffer::Samples,
                    requested_capacity: sample_count,
                }
            })?;

            for &interval in record.intervals_seconds() {
                interval_bits.push(interval.to_bits());
            }
            for &sample in record.samples() {
                sample_bits.push(sample.to_bits());
            }

            Ok(RecordBits::F64 {
                cutoff_bits: record.cutoff_hz().to_bits(),
                interval_bits,
                sample_bits,
            })
        }
    }
}

pub(crate) fn score_response(
    order: FilterOrder,
    record: &RepresentedRecord,
    response: forward_backward_protocol::CaseResult,
) -> Result<WholeRecordAssessment, CellExecutionError> {
    let candidate = materialize_candidate_result(response, record)?;
    let order = match order {
        FilterOrder::One => 1,
        FilterOrder::Two => 2,
        FilterOrder::Four => 4,
        FilterOrder::Eight => 8,
    };
    Ok(score_whole_record(order, record, candidate)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    use forward_backward_protocol::{
        CandidateErrorCategory, CandidateErrorResponse, CaseIdentity, CaseResult, OutputBits, Pass,
        SuccessResponse, TailEvidence,
    };

    use crate::candidate::CandidateOutcome;
    use crate::identity::CellVariant;
    use crate::score::CandidateDisposition;
    use crate::{
        CaseCoordinate, FixtureCoordinate, PhysicalCutoff, QMax, ScheduleKind, TimeDomainFixture,
        ToneCoordinate, ToneFrequency, ToneOrder, TonePhase,
    };

    fn fixture_key(
        scalar: RecordScalar,
        cutoff: PhysicalCutoff,
        filter_order: FilterOrder,
        variant: CellVariant,
    ) -> ResponseCellKey {
        ResponseCellKey::new(
            CaseCoordinate::TimeDomain(FixtureCoordinate {
                fixture: TimeDomainFixture::UnequalMultitone,
                q_max: QMax::PiOver128,
                schedule: ScheduleKind::Uniform,
            }),
            scalar,
            cutoff,
            filter_order,
            variant,
        )
    }

    fn tone_key(filter_order: FilterOrder) -> ResponseCellKey {
        ResponseCellKey::new(
            CaseCoordinate::Tone(ToneCoordinate {
                order: ToneOrder::One,
                frequency: ToneFrequency::OneSixteenth,
                phase: TonePhase::SeventeenHundredths,
                q_max: QMax::PiOver128,
                schedule: ScheduleKind::Uniform,
            }),
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            filter_order,
            CellVariant::Base,
        )
    }

    fn constant_record() -> RepresentedRecord {
        RepresentedRecord::from_f64(1.0, vec![0.125], vec![0.5, 0.5]).unwrap()
    }

    fn response_identity(record: &RepresentedRecord, order: FilterOrder) -> CaseIdentity {
        let key = fixture_key(
            record.scalar(),
            PhysicalCutoff::OneHz,
            order,
            CellVariant::Base,
        );
        identity_for(&key, record).unwrap()
    }

    fn assert_protocol_valid(
        record: &RepresentedRecord,
        order: FilterOrder,
        response: &CaseResult,
    ) {
        let key = fixture_key(
            record.scalar(),
            PhysicalCutoff::OneHz,
            order,
            CellVariant::Base,
        );
        let request = build_case_request(&key, record).unwrap();
        forward_backward_protocol::validate_case_result(&request, response)
            .expect("test response must match the request identity and shape");
    }

    fn success(
        record: &RepresentedRecord,
        order: FilterOrder,
        output_bits: OutputBits,
    ) -> CaseResult {
        let response = CaseResult::Success(SuccessResponse {
            identity: response_identity(record, order),
            output_bits,
            tail_evidence: TailEvidence::Omitted,
            forward: Default::default(),
            tail: Default::default(),
            reverse: Default::default(),
        });
        assert_protocol_valid(record, order, &response);
        response
    }

    fn candidate_error(record: &RepresentedRecord, order: FilterOrder) -> CaseResult {
        let response = CaseResult::CandidateError(CandidateErrorResponse {
            identity: response_identity(record, order),
            category: CandidateErrorCategory::Integration,
            pass: Some(Pass::Reverse),
            interval: Some(0),
            source_category: None,
        });
        assert_protocol_valid(record, order, &response);
        response
    }

    #[test]
    fn f32_request_preserves_native_bits_and_identity() {
        let minimum_subnormal = f32::from_bits(1);
        let record = RepresentedRecord::from_f32(
            1.0,
            vec![minimum_subnormal],
            vec![-0.0, minimum_subnormal],
        )
        .unwrap();
        let key = fixture_key(
            RecordScalar::F32,
            PhysicalCutoff::OneHz,
            FilterOrder::Four,
            CellVariant::Base,
        );
        let expected_identity = identity_for(&key, &record).unwrap();

        let request = build_case_request(&key, &record).unwrap();

        assert_eq!(request.identity, expected_identity);
        assert_eq!(request.order, FilterOrder::Four);
        assert_eq!(
            request.record,
            RecordBits::F32 {
                cutoff_bits: 1.0_f32.to_bits(),
                interval_bits: vec![1],
                sample_bits: vec![0x8000_0000, 1],
            }
        );
    }

    #[test]
    fn f64_request_preserves_native_bits_and_identity() {
        let minimum_subnormal = f64::from_bits(1);
        let record = RepresentedRecord::from_f64(
            37.0,
            vec![minimum_subnormal],
            vec![-0.0, minimum_subnormal],
        )
        .unwrap();
        let key = fixture_key(
            RecordScalar::F64,
            PhysicalCutoff::ThirtySevenHz,
            FilterOrder::Eight,
            CellVariant::Base,
        );
        let expected_identity = identity_for(&key, &record).unwrap();

        let request = build_case_request(&key, &record).unwrap();

        assert_eq!(request.identity, expected_identity);
        assert_eq!(request.order, FilterOrder::Eight);
        assert_eq!(
            request.record,
            RecordBits::F64 {
                cutoff_bits: 37.0_f64.to_bits(),
                interval_bits: vec![1],
                sample_bits: vec![0x8000_0000_0000_0000, 1],
            }
        );
    }

    #[test]
    fn variant_changes_case_id_without_changing_record_digest() {
        let record = RepresentedRecord::from_f64(1.0, vec![0.125], vec![0.0, 1.0]).unwrap();
        let base = fixture_key(
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            FilterOrder::One,
            CellVariant::Base,
        );
        let refined = fixture_key(
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            FilterOrder::One,
            CellVariant::ScalarRefined,
        );

        let base_request = build_case_request(&base, &record).unwrap();
        let refined_request = build_case_request(&refined, &record).unwrap();

        assert_ne!(
            base_request.identity.case_id,
            refined_request.identity.case_id
        );
        assert_eq!(
            base_request.identity.record_digest,
            refined_request.identity.record_digest
        );
    }

    #[test]
    fn identity_mismatches_precede_request_allocation() {
        let f64_record = RepresentedRecord::from_f64(1.0, vec![], vec![0.5]).unwrap();
        let scalar_mismatch = fixture_key(
            RecordScalar::F32,
            PhysicalCutoff::OneHz,
            FilterOrder::One,
            CellVariant::Base,
        );
        assert!(matches!(
            build_case_request(&scalar_mismatch, &f64_record),
            Err(RequestBuildError::Identity(IdentityError::ScalarMismatch {
                key_scalar: RecordScalar::F32,
                record_scalar: RecordScalar::F64,
            }))
        ));

        let cutoff_record = RepresentedRecord::from_f64(37.0, vec![], vec![0.5]).unwrap();
        let cutoff_mismatch = fixture_key(
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            FilterOrder::One,
            CellVariant::Base,
        );
        assert!(matches!(
            build_case_request(&cutoff_mismatch, &cutoff_record),
            Err(RequestBuildError::Identity(IdentityError::CutoffMismatch {
                key_cutoff: PhysicalCutoff::OneHz,
                ..
            }))
        ));

        let tone_record = RepresentedRecord::from_f64(1.0, vec![], vec![0.5]).unwrap();
        assert!(matches!(
            build_case_request(&tone_key(FilterOrder::Eight), &tone_record),
            Err(RequestBuildError::Identity(
                IdentityError::ToneOrderMismatch {
                    coordinate_order: ToneOrder::One,
                    filter_order: FilterOrder::Eight,
                }
            ))
        ));
    }

    #[test]
    fn scoring_retains_exact_promoted_signed_zero_and_subnormal_output() {
        let minimum_subnormal = f64::from_bits(1);
        let record =
            RepresentedRecord::from_f64(1.0, vec![0.125], vec![-0.0, minimum_subnormal]).unwrap();

        let assessment = score_response(
            FilterOrder::One,
            &record,
            success(
                &record,
                FilterOrder::One,
                OutputBits::F64(vec![0x8000_0000_0000_0000, 1]),
            ),
        )
        .unwrap();

        let output = assessment
            .scored_output()
            .expect("finite output is retained");
        assert_eq!(output.len(), 2);
        assert!(output.iter().all(|value| value.prec() == 256));
        assert_eq!(output[0].to_f64().to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(output[1].to_f64().to_bits(), 1);
        let CandidateDisposition::Scored { gates, .. } = assessment.candidate() else {
            panic!("expected a finite scored candidate");
        };
        assert!(gates.passed());
    }

    #[test]
    fn candidate_errors_non_finite_results_and_gate_failures_are_collectable() {
        let record = constant_record();
        let error_assessment = score_response(
            FilterOrder::One,
            &record,
            candidate_error(&record, FilterOrder::One),
        )
        .expect("candidate failure remains a cell outcome");
        assert!(matches!(
            error_assessment.candidate(),
            CandidateDisposition::CandidateError(_)
        ));
        assert!(error_assessment.scored_output().is_none());

        let non_finite_assessment = score_response(
            FilterOrder::One,
            &record,
            success(
                &record,
                FilterOrder::One,
                OutputBits::F64(vec![f64::NAN.to_bits(), 0.5_f64.to_bits()]),
            ),
        )
        .expect("non-finite success remains a cell outcome");
        assert!(matches!(
            non_finite_assessment.candidate(),
            CandidateDisposition::NonFiniteSuccess(_)
        ));

        let gate_failure = score_response(
            FilterOrder::One,
            &record,
            success(
                &record,
                FilterOrder::One,
                OutputBits::F64(vec![2.0_f64.to_bits(), 2.0_f64.to_bits()]),
            ),
        )
        .expect("failed numerical gates remain a scored cell outcome");
        let CandidateDisposition::Scored { gates, .. } = gate_failure.candidate() else {
            panic!("expected a finite scored candidate");
        };
        assert!(!gates.passed());
    }

    #[test]
    fn collectable_first_result_does_not_block_a_later_valid_result() {
        let record = constant_record();

        let first = score_response(
            FilterOrder::One,
            &record,
            candidate_error(&record, FilterOrder::One),
        )
        .unwrap();
        assert!(matches!(
            first.candidate(),
            CandidateDisposition::CandidateError(_)
        ));

        let second = score_response(
            FilterOrder::One,
            &record,
            success(
                &record,
                FilterOrder::One,
                OutputBits::F64(vec![0.5_f64.to_bits(), 0.5_f64.to_bits()]),
            ),
        )
        .unwrap();
        let CandidateDisposition::Scored { gates, .. } = second.candidate() else {
            panic!("expected the later valid result to be scored");
        };
        assert!(gates.passed());
    }

    #[test]
    fn fatal_errors_keep_request_materialization_and_scoring_categories() {
        let identity_error = RequestBuildError::Identity(IdentityError::CutoffMismatch {
            key_cutoff: PhysicalCutoff::OneHz,
            record_cutoff_hz: 37.0,
        });
        let encoding_error = RequestBuildError::Encoding(IdentityError::NonExactF32 {
            field: crate::identity::RecordValueField::Sample,
            index: Some(0),
            value: 0.1_f64,
        });
        let allocation_error = RequestBuildError::Allocation {
            buffer: RequestBuffer::Samples,
            requested_capacity: 2,
        };
        assert!(matches!(
            CellExecutionError::from(identity_error),
            CellExecutionError::Request(RequestBuildError::Identity(_))
        ));
        assert!(matches!(
            CellExecutionError::from(encoding_error),
            CellExecutionError::Request(RequestBuildError::Encoding(_))
        ));
        assert!(matches!(
            CellExecutionError::from(allocation_error),
            CellExecutionError::Request(RequestBuildError::Allocation { .. })
        ));

        let materialization_error = CandidateMaterializationError::OutputLengthMismatch {
            expected: 1,
            actual: 0,
        };
        assert!(matches!(
            CellExecutionError::from(materialization_error),
            CellExecutionError::Materialization(_)
        ));

        let record = constant_record();
        let CaseResult::CandidateError(candidate_error) =
            candidate_error(&record, FilterOrder::One)
        else {
            unreachable!("helper returns a candidate error");
        };
        let scoring_error = score_whole_record(
            3,
            &record,
            CandidateOutcome::CandidateError(candidate_error),
        )
        .unwrap_err();
        assert!(matches!(
            CellExecutionError::from(scoring_error),
            CellExecutionError::Scoring(WholeRecordFatalError::OracleIntegrity(_))
        ));
    }
}
