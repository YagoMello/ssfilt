//! Continuous-time filters for irregularly sampled signals.
//!
//! `ssfilt` advances a continuous-time filter model by the elapsed time supplied
//! with each sample. The runtime is allocation-free and supports `no_std`.
//!
//! # Quick start
//!
//! Initialize from the first known sample, then pass the actual elapsed time
//! between each pair of timestamps:
//!
//! ```
//! use ssfilt::{LowPass, Response};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let samples = [(0.0, 22.18), (0.18, 22.12), (0.51, 22.25)];
//! let mut filter = LowPass::<2>::builder(0.7)
//!     .response(Response::Butterworth)
//!     .initial_input(samples[0].1)
//!     .build()?;
//!
//! let mut previous_time = samples[0].0;
//! for &(time, input) in &samples[1..] {
//!     let dt_seconds = time - previous_time;
//!     let output = filter.update(input, dt_seconds)?;
//!     assert!(output.is_finite());
//!     previous_time = time;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`LowPass`], [`HighPass`], and [`BandPass`] share the same builder and
//! streaming conventions. Each cutoff denotes a complete-response −3 dB
//! frequency. [`InputModel`] defines the signal reconstruction inside each
//! elapsed interval; failed updates leave the filter unchanged.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("enable either the `std` or `libm` feature");

mod band_pass;
mod config;
mod diagnostics;
mod error;
mod high_pass;
mod low_pass;
mod model;
mod response;
mod scalar;
mod solver;
mod state;
mod streaming;

pub use band_pass::{BandPass, BandPassBuilder};
pub use config::{InputModel, IntegrationConfig, Tolerances};
pub use diagnostics::IntegrationDiagnostics;
pub use error::{BuildError, ResetError, UpdateError};
pub use high_pass::{HighPass, HighPassBuilder};
pub use low_pass::{LowPass, LowPassBuilder, PhaseEqualizedLowPass};
pub use response::{MAX_BESSEL_BAND_PASS_ORDER, MAX_BESSEL_ORDER, Response};
pub use scalar::Scalar;

/// Common behavior for a stateful streaming filter.
pub trait StreamingFilter {
    /// Numeric scalar used by this filter.
    type Scalar: crate::Scalar;

    /// Advances the filter by `dt_seconds` and returns the new output.
    ///
    /// The interpretation of `input` over the elapsed interval is selected by
    /// [`InputModel`]. The returned output is evaluated at the newly supplied
    /// endpoint input, which matters for filters with direct feedthrough.
    /// Failed updates leave the filter unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] when the input or elapsed time is invalid, or
    /// when integration cannot complete within its safety limits.
    fn update(
        &mut self,
        input: Self::Scalar,
        dt_seconds: Self::Scalar,
    ) -> Result<Self::Scalar, UpdateError>;

    /// Returns the most recently accepted output.
    fn output(&self) -> Self::Scalar;

    /// Returns the filter to zero-input steady state.
    fn reset(&mut self);

    /// Sets the state as if `input` had been constant for an infinite time.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError::NonFiniteInput`] when `input` is not finite, or
    /// [`ResetError::UnrepresentableSteadyState`] when its finite equilibrium
    /// cannot be represented.
    fn reset_to_steady(&mut self, input: Self::Scalar) -> Result<(), ResetError>;
}
