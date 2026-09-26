//! Continuous-time filters for irregularly sampled signals.
//!
//! `ssfilt` advances a continuous-time filter model by the elapsed time supplied
//! with each sample. The runtime is allocation-free and supports `no_std`.
//! Offline forward-backward filtering and delayed low-pass phase equalization
//! provide separate ways to handle phase-sensitive signals.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("enable either the `std` or `libm` feature");

mod band_pass;
mod batch;
mod config;
mod diagnostics;
mod error;
mod high_pass;
mod low_pass;
mod model;
mod phase;
mod response;
mod scalar;
mod solver;
mod streaming;

pub use band_pass::{BandPass, BandPassBuilder};
pub use batch::{BatchError, forward_backward_into, forward_backward_uniform_into};
pub use config::{InputModel, IntegrationConfig, Tolerances};
pub use diagnostics::IntegrationDiagnostics;
pub use error::{BuildError, ResetError, UpdateError};
pub use high_pass::{HighPass, HighPassBuilder};
pub use low_pass::{LowPass, LowPassBuilder};
pub use phase::{PhaseEqualizationError, PhaseEqualizedLowPass};
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
    /// Returns [`ResetError`] when `input` is not finite.
    fn reset_to_steady(&mut self, input: Self::Scalar) -> Result<(), ResetError>;
}
