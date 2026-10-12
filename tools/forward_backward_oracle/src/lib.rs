#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod fixtures;
mod model;
mod oracle;
mod record;
mod represented;
mod segment;
mod session;

pub use fixtures::{
    FixtureBuildError, FixtureCoordinate, NominalRecord, Q_MAXES, QMax, SCHEDULES, ScheduleKind,
    TIME_DOMAIN_FIXTURES, TimeDomainFixture, analytic_fixture_value, build_nominal_record,
};
pub use model::ButterworthModes;
pub use oracle::{OracleOutput, evaluate};
pub use record::{RecordError, RecordScalar, RepresentedRecord};
pub use represented::{
    PHYSICAL_CUTOFFS, PhysicalCutoff, RepresentedBuildError, RepresentedBuildErrorKind,
    RepresentedCase, SCALAR_ORDER, ScalarBits, build_represented_case,
};
pub use session::run_session_smoke;
