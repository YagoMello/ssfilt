#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod assessment;
// This module is staged for the next scorer; remove the allowance when it consumes these types.
#[allow(dead_code)]
mod candidate;
mod case;
mod fixtures;
mod model;
mod oracle;
mod record;
mod refinement;
mod represented;
mod segment;
mod session;
mod tones;

pub use assessment::{
    ArithmeticOperation, AssessmentBuffer, AssessmentError, AssessmentErrorKind, CertificationGate,
    CertifiedOracle, DurationIssue, ErrorMetrics, HIGH_ORACLE_PRECISION, LOW_ORACLE_PRECISION,
    MINIMUM_ASSESSMENT_PRECISION, OracleAssessment, OracleSide, ReversedRecord, SeriesSide,
    assess_oracle, certify_oracle, extract_original_knots, reverse_record, weighted_error,
};
pub use case::CaseCoordinate;
pub use fixtures::{
    FixtureBuildError, FixtureCoordinate, NominalRecord, Q_MAXES, QMax, SCHEDULES, ScheduleKind,
    TIME_DOMAIN_FIXTURES, TimeDomainFixture, analytic_fixture_value, build_nominal_record,
};
pub use model::ButterworthModes;
pub use oracle::{OracleOutput, evaluate};
pub use record::{RecordError, RecordScalar, RepresentedRecord};
pub use refinement::{
    RefinedBuffer, RepresentedValueField, ScalarRefinement, ScalarRefinementError,
    ScalarRefinementErrorKind, SplitPart, refine_represented_case,
};
pub use represented::{
    PHYSICAL_CUTOFFS, PhysicalCutoff, RepresentedBuildError, RepresentedBuildErrorKind,
    RepresentedCase, SCALAR_ORDER, ScalarBits, build_represented_case, build_represented_tone_case,
};
pub use session::run_session_smoke;
pub use tones::{
    NominalToneRecord, TONE_FREQUENCIES, TONE_ORDERS, TONE_PHASES, TONE_RECORD_COUNT,
    ToneBuildError, ToneBuildErrorKind, ToneCoordinate, ToneFitMetadata, ToneFrequency, ToneOrder,
    TonePhase, ToneRecordBuffer, build_nominal_tone_record,
};
