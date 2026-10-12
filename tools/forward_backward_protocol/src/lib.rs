#![forbid(unsafe_code)]

//! Standalone facade for the forward-backward acceptance-harness protocol.

#[path = "../../../src/batch_protocol.rs"]
mod implementation;

pub use implementation::*;
