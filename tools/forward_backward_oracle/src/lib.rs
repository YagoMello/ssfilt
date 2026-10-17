#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod assessment;
// Candidate materialization is ready for the scorer; both await the matrix runner.
#[allow(dead_code)]
mod candidate;
mod case;
// Cross-cell acceptance checks are consumed by the future matrix runner.
#[allow(dead_code)]
mod comparison;
mod fixtures;
mod harness;
// The matrix runner will consume these frozen identities in a later stage.
#[allow(dead_code)]
mod identity;
mod manifest;
// The matrix runner will consume this private single-cell seam in a later stage.
#[allow(dead_code)]
mod matrix;
mod model;
mod oracle;
mod provenance;
mod record;
mod refinement;
mod represented;
// Whole-record scoring is staged until the matrix runner consumes its results.
mod report;
#[allow(dead_code)]
mod score;
mod segment;
mod selection;
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
pub use harness::run_filtered_matrix;
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
pub use selection::CandidateKind;
pub use session::run_session_smoke;
pub use tones::{
    NominalToneRecord, TONE_FREQUENCIES, TONE_ORDERS, TONE_PHASES, TONE_RECORD_COUNT,
    ToneBuildError, ToneBuildErrorKind, ToneCoordinate, ToneFitMetadata, ToneFrequency, ToneOrder,
    TonePhase, ToneRecordBuffer, build_nominal_tone_record,
};
