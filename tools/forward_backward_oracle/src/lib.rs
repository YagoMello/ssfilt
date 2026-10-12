#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod fixtures;
mod model;
mod oracle;
mod record;
mod segment;
mod session;

pub use fixtures::{
    FixtureBuildError, FixtureCoordinate, NominalRecord, Q_MAXES, QMax, SCHEDULES, ScheduleKind,
    TIME_DOMAIN_FIXTURES, TimeDomainFixture, analytic_fixture_value, build_nominal_record,
};
pub use model::ButterworthModes;
pub use oracle::{OracleOutput, evaluate};
pub use record::{RecordError, RecordScalar, RepresentedRecord};
pub use session::run_session_smoke;
