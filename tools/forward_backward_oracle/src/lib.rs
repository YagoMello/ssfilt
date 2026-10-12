#![forbid(unsafe_code)]

//! Standalone development support for the forward-backward reference oracle.

mod model;
mod oracle;
mod record;
mod segment;

pub use model::ButterworthModes;
pub use oracle::{OracleOutput, evaluate};
pub use record::{RecordError, RecordScalar, RepresentedRecord};
