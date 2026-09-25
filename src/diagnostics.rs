use crate::Scalar;
use crate::solver::SolverDiagnostics;

/// Work performed by the most recent successful filter update.
///
/// The counters describe the adaptive integration work for one call to
/// `update`. They are observations rather than configuration: solver details
/// may evolve without changing the filter's numerical contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntegrationDiagnostics<T> {
    accepted_steps: usize,
    rejected_steps: usize,
    derivative_evaluations: usize,
    smallest_accepted_step_seconds: Option<T>,
    largest_accepted_step_seconds: Option<T>,
    equilibrium_shortcut: bool,
}

impl<T> IntegrationDiagnostics<T> {
    pub(crate) const fn empty() -> Self {
        Self {
            accepted_steps: 0,
            rejected_steps: 0,
            derivative_evaluations: 0,
            smallest_accepted_step_seconds: None,
            largest_accepted_step_seconds: None,
            equilibrium_shortcut: false,
        }
    }

    /// Returns the number of internal steps that advanced the state.
    #[must_use]
    pub const fn accepted_steps(&self) -> usize {
        self.accepted_steps
    }

    /// Returns the number of internal steps rejected by error control.
    #[must_use]
    pub const fn rejected_steps(&self) -> usize {
        self.rejected_steps
    }

    /// Returns the total number of accepted and rejected step attempts.
    #[must_use]
    pub const fn attempted_steps(&self) -> usize {
        self.accepted_steps.saturating_add(self.rejected_steps)
    }

    /// Returns the number of model derivative evaluations.
    ///
    /// This is a direct measure of computational work, but callers should not
    /// assume a fixed number of evaluations per step.
    #[must_use]
    pub const fn derivative_evaluations(&self) -> usize {
        self.derivative_evaluations
    }

    /// Returns the smallest accepted internal step in seconds.
    ///
    /// This is `None` when no integration step was needed.
    #[must_use]
    pub const fn smallest_accepted_step_seconds(&self) -> Option<T>
    where
        T: Copy,
    {
        self.smallest_accepted_step_seconds
    }

    /// Returns the largest accepted internal step in seconds.
    ///
    /// This is `None` when no integration step was needed.
    #[must_use]
    pub const fn largest_accepted_step_seconds(&self) -> Option<T>
    where
        T: Copy,
    {
        self.largest_accepted_step_seconds
    }

    /// Returns whether an exact constant-input equilibrium avoided integration.
    #[must_use]
    pub const fn used_equilibrium_shortcut(&self) -> bool {
        self.equilibrium_shortcut
    }
}

impl<T: Scalar> IntegrationDiagnostics<T> {
    pub(crate) fn from_solver(diagnostics: SolverDiagnostics<T>, angular_cutoff: T) -> Self {
        Self {
            accepted_steps: diagnostics.accepted_steps,
            rejected_steps: diagnostics.rejected_steps,
            derivative_evaluations: diagnostics.derivative_evaluations,
            smallest_accepted_step_seconds: diagnostics
                .smallest_accepted_step
                .map(|step| step / angular_cutoff),
            largest_accepted_step_seconds: diagnostics
                .largest_accepted_step
                .map(|step| step / angular_cutoff),
            equilibrium_shortcut: diagnostics.equilibrium_shortcut,
        }
    }
}

impl<T> Default for IntegrationDiagnostics<T> {
    fn default() -> Self {
        Self::empty()
    }
}
