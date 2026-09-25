use crate::Real;

/// Assumption made about the input between consecutive samples.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum InputModel {
    /// Linearly interpolate from the preceding input to the current input.
    #[default]
    Linear,
    /// Hold the preceding input through the elapsed interval.
    PreviousHold,
    /// Hold the current input through the elapsed interval.
    CurrentHold,
}

/// Absolute and relative local-error tolerances for adaptive integration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerances<T> {
    /// Absolute error floor applied to each state component.
    pub absolute: T,
    /// Error allowed relative to each state component's magnitude.
    pub relative: T,
}

impl<T: Real> Tolerances<T> {
    /// Creates a tolerance pair.
    #[must_use]
    pub const fn new(absolute: T, relative: T) -> Self {
        Self { absolute, relative }
    }

    pub(crate) fn is_valid(self) -> bool {
        self.absolute.is_finite()
            && self.relative.is_finite()
            && self.absolute > T::zero()
            && self.relative > T::zero()
    }
}

impl<T: Real> Default for Tolerances<T> {
    fn default() -> Self {
        Self::new(
            T::default_absolute_tolerance(),
            T::default_relative_tolerance(),
        )
    }
}

/// Safety and accuracy controls for adaptive integration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntegrationConfig<T> {
    /// Local error tolerances.
    pub tolerances: Tolerances<T>,
    /// Optional upper bound for each internal integration step, in seconds.
    ///
    /// This subdivides the complete sample interval; it never discards elapsed
    /// time. The filter also applies a model-derived stability bound.
    pub max_step_seconds: Option<T>,
    /// Maximum number of accepted and rejected internal step attempts.
    pub max_step_attempts: usize,
}

impl<T: Real> IntegrationConfig<T> {
    pub(crate) fn is_valid(self) -> bool {
        self.tolerances.is_valid()
            && self
                .max_step_seconds
                .is_none_or(|step| step.is_finite() && step > T::zero())
            && self.max_step_attempts > 0
    }
}

impl<T: Real> Default for IntegrationConfig<T> {
    fn default() -> Self {
        Self {
            tolerances: Tolerances::default(),
            max_step_seconds: None,
            max_step_attempts: 4_096,
        }
    }
}
