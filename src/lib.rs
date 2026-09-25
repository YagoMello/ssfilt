//! Continuous-time filters for irregularly sampled signals.
//!
//! `ssfilt` advances a continuous-time filter model by the elapsed time supplied
//! with each sample. The runtime is allocation-free and supports `no_std`.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("enable either the `std` or `libm` feature");

mod config;
mod error;
mod low_pass;
mod model;
mod response;
mod scalar;
mod solver;

pub use config::{InputModel, IntegrationConfig, Tolerances};
pub use error::{BuildError, ResetError, UpdateError};
pub use low_pass::{LowPass, LowPassBuilder};
pub use response::Response;
pub use scalar::Scalar;

/// Common behavior for a stateful streaming filter.
pub trait StreamingFilter {
    /// Numeric scalar used by this filter.
    type Scalar: crate::Scalar;

    /// Advances the filter by `dt_seconds` and returns the new output.
    ///
    /// The interpretation of `input` over the elapsed interval is selected by
    /// [`InputModel`]. Failed updates leave the filter unchanged.
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
