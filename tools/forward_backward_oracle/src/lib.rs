#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod model;
mod oracle;
mod record;
mod segment;
mod session;

pub use model::ButterworthModes;
pub use oracle::{OracleOutput, evaluate};
pub use record::{RecordError, RecordScalar, RepresentedRecord};
pub use session::run_session_smoke;
