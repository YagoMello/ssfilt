use core::fmt;

/// Error returned while constructing a filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BuildError {
    /// Compile-time filter order was zero.
    ZeroOrder,
    /// Cutoff frequency was non-finite or not positive.
    InvalidCutoff,
    /// Initial input was not finite.
    InvalidInitialInput,
    /// Integration configuration contained an invalid value.
    InvalidIntegrationConfig,
    /// Chebyshev passband ripple was outside its supported range.
    InvalidPassbandRipple,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroOrder => "filter order must be at least one",
            Self::InvalidCutoff => "cutoff frequency must be finite and positive",
            Self::InvalidInitialInput => "initial input must be finite",
            Self::InvalidIntegrationConfig => "integration configuration is invalid",
            Self::InvalidPassbandRipple => {
                "passband ripple must be finite, positive, and less than 3.0103 dB"
            }
        };
        formatter.write_str(message)
    }
}

/// Error returned while advancing a filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum UpdateError {
    /// The supplied input was NaN or infinite.
    NonFiniteInput,
    /// Elapsed time was NaN, infinite, zero, or negative.
    InvalidDeltaTime,
    /// Adaptive integration exhausted its configured work budget.
    StepBudgetExceeded,
    /// Floating-point resolution prevented further progress.
    StepSizeUnderflow,
    /// The candidate state became non-finite even after reducing the step.
    NonFiniteState,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NonFiniteInput => "input must be finite",
            Self::InvalidDeltaTime => "elapsed time must be finite and positive",
            Self::StepBudgetExceeded => "adaptive integration exhausted its step budget",
            Self::StepSizeUnderflow => "integration step is too small to advance time",
            Self::NonFiniteState => "integration produced a non-finite state",
        };
        formatter.write_str(message)
    }
}

/// Error returned while resetting a filter to a steady input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResetError {
    /// The supplied steady input was NaN or infinite.
    NonFiniteInput,
}

impl fmt::Display for ResetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("steady input must be finite")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BuildError {}

#[cfg(feature = "std")]
impl std::error::Error for UpdateError {}

#[cfg(feature = "std")]
impl std::error::Error for ResetError {}
