//! Continuous-time filters for irregularly sampled signals.
//!
//! `ssfilt` advances a continuous-time filter model by the elapsed time supplied
//! with each sample. It provides allocation-free [`LowPass`], [`HighPass`], and
//! [`BandPass`] filters and supports `no_std`. This prerelease API is intended
//! for field testing rather than a production-readiness claim.
//!
//! # Quick start
//!
//! The new input is the sample at the end of `dt_seconds`:
//!
//! ```
//! use ssfilt::{InputModel, LowPass, Response};
//!
//! let mut filter = LowPass::<4>::builder(20.0)
//!     .response(Response::Butterworth)
//!     .input_model(InputModel::Linear)
//!     .initial_input(0.0)
//!     .build()?;
//!
//! let output = filter.update(1.0, 0.012)?;
//! assert!(output.is_finite());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [`InputModel`] defines what is assumed between the preceding and new
//! endpoints. Supply the complete measured interval; updates integrate all of
//! it or fail transactionally without changing the filter.
//!
//! # Choosing a filter
//!
//! - [`LowPass`] smooths high-frequency variation.
//! - [`HighPass`] removes slow baseline drift and has endpoint direct
//!   feedthrough.
//! - [`BandPass`] keeps a range between two complete-response −3 dB edges. Its
//!   final order must be positive and even.
//!
//! Every public cutoff denotes the complete filter's −3 dB frequency. The
//! [`Response`] selects repeated-pole, Butterworth, Bessel, or Chebyshev Type I
//! behavior without changing that convention.
//!
//! # Embedded use
//!
//! Disable default features and enable `libm` for an allocation-free `no_std`
//! build. Validated scalar types are `f32` and `f64`.
//!
//! Streaming phase correction and offline zero-phase filtering are not part of
//! this release. Exact zero-phase streaming would require future samples or
//! added latency; recorded data can instead use a separate offline
//! forward-backward or frequency-domain workflow.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

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
mod streaming;

pub use band_pass::{BandPass, BandPassBuilder};
pub use config::{InputModel, IntegrationConfig, Tolerances};
pub use diagnostics::IntegrationDiagnostics;
pub use error::{BuildError, ResetError, UpdateError};
pub use high_pass::{HighPass, HighPassBuilder};
pub use low_pass::{LowPass, LowPassBuilder};
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
