use core::fmt::Debug;

use crate::model::ContinuousModel;
use crate::solver::InputSegment;
use crate::{InputModel, Scalar, UpdateError};

/// A supported integration policy for an order-`N` filter.
///
/// This trait is sealed. Applications select one of the integration policies
/// supplied by `ssfilt`; the continuous model and numerical execution
/// interface remain private.
pub trait Integrator<T: Scalar, const N: usize>:
    private::IntegratorImpl<T, N> + Copy + Debug
{
}

impl<T, I, const N: usize> Integrator<T, N> for I
where
    T: Scalar,
    I: private::IntegratorImpl<T, N> + Copy + Debug,
{
}

/// Adaptive Dormand–Prince 5(4) integration.
///
/// This is the default integration policy. Tolerances describe local state
/// error, not a direct bound on output error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DormandPrince54<T> {
    /// Absolute local-state error tolerance.
    pub absolute_tolerance: T,
    /// Relative local-state error tolerance.
    pub relative_tolerance: T,
    /// Optional physical upper bound for each internal step, in seconds.
    ///
    /// This subdivides the complete sample interval; it never discards elapsed
    /// time. A private model-derived stability bound may subdivide it further.
    pub max_step_seconds: Option<T>,
    /// Maximum number of accepted and rejected internal step attempts.
    pub max_step_attempts: usize,
}

impl Default for DormandPrince54<f32> {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1.0e-6,
            relative_tolerance: 1.0e-4,
            max_step_seconds: None,
            max_step_attempts: 4_096,
        }
    }
}

impl Default for DormandPrince54<f64> {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1.0e-10,
            relative_tolerance: 1.0e-7,
            max_step_seconds: None,
            max_step_attempts: 4_096,
        }
    }
}

pub(crate) mod private {
    use core::fmt::Debug;

    use crate::model::ContinuousModel;
    use crate::solver::{InputSegment, SolverDiagnostics};
    use crate::{InputModel, Scalar, UpdateError};

    /// Internal numerical integration contract.
    ///
    /// This trait is deliberately kept behind the sealed public `Integrator`
    /// trait. Its runtime type, continuous-model access, derivative interface,
    /// normalized-time request, candidate state and cache, diagnostics, and
    /// failures are implementation details. Exposing them would turn `ssfilt`
    /// into a general ODE framework and prevent those internals from evolving.
    ///
    /// Keep this module inaccessible outside the crate. Do not re-export this
    /// trait or its associated runtime types.
    ///
    /// Rust permits downstream generic code to refer abstractly to the
    /// inherited `Runtime` associated type. Its concrete type, construction,
    /// fields, and operations remain inaccessible; the hook exists solely to
    /// give each sealed policy fixed-size, order-specific storage.
    pub trait IntegratorImpl<T: Scalar, const N: usize>: Copy + Debug {
        type Runtime: Copy + Debug;

        fn is_valid(self, token: Token) -> bool;

        fn into_runtime<M: ContinuousModel<T, N>>(
            self,
            token: Token,
            model: &M,
            state: &[T; N],
            input: T,
            input_model: InputModel,
        ) -> Self::Runtime;

        fn reset_runtime<M: ContinuousModel<T, N>>(
            token: Token,
            runtime: &Self::Runtime,
            model: &M,
            state: &[T; N],
            input: T,
            input_model: InputModel,
        ) -> Self::Runtime;

        // The arguments deliberately keep the numerical request explicit and
        // allocation-free. This is a private implementation hook, not an API
        // callers must construct.
        #[allow(clippy::too_many_arguments)]
        fn integrate<M: ContinuousModel<T, N>>(
            token: Token,
            runtime: &Self::Runtime,
            model: &M,
            initial_state: &[T; N],
            input: InputSegment<T>,
            input_model: InputModel,
            normalized_duration: T,
            time_scale: T,
        ) -> Result<IntegrationCandidate<T, N, Self::Runtime>, UpdateError>;
    }

    /// Inaccessible capability required to invoke implementation hooks.
    ///
    /// Downstream generic code can name the sealed public bound, but cannot
    /// construct this token and therefore cannot execute the numerical
    /// implementation interface inherited from the private supertrait.
    #[derive(Clone, Copy, Debug)]
    pub struct Token;

    #[derive(Clone, Copy, Debug)]
    pub struct IntegrationCandidate<T, const N: usize, R> {
        pub(crate) state: [T; N],
        pub(crate) runtime: R,
        pub(crate) diagnostics: SolverDiagnostics<T>,
    }
}

impl<T: Scalar, const N: usize> private::IntegratorImpl<T, N> for DormandPrince54<T> {
    type Runtime = crate::solver::DormandPrinceRuntime<T, N>;

    fn is_valid(self, _token: private::Token) -> bool {
        self.absolute_tolerance.is_finite()
            && self.relative_tolerance.is_finite()
            && self.absolute_tolerance > T::zero()
            && self.relative_tolerance > T::zero()
            && self
                .max_step_seconds
                .is_none_or(|step| step.is_finite() && step > T::zero())
            && self.max_step_attempts > 0
    }

    fn into_runtime<M: ContinuousModel<T, N>>(
        self,
        _token: private::Token,
        model: &M,
        state: &[T; N],
        input: T,
        input_model: InputModel,
    ) -> Self::Runtime {
        crate::solver::DormandPrinceRuntime::new(self, model, state, input, input_model)
    }

    fn reset_runtime<M: ContinuousModel<T, N>>(
        _token: private::Token,
        runtime: &Self::Runtime,
        model: &M,
        state: &[T; N],
        input: T,
        input_model: InputModel,
    ) -> Self::Runtime {
        crate::solver::DormandPrinceRuntime::new(runtime.policy, model, state, input, input_model)
    }

    fn integrate<M: ContinuousModel<T, N>>(
        _token: private::Token,
        runtime: &Self::Runtime,
        model: &M,
        initial_state: &[T; N],
        input: InputSegment<T>,
        input_model: InputModel,
        normalized_duration: T,
        time_scale: T,
    ) -> Result<private::IntegrationCandidate<T, N, Self::Runtime>, UpdateError> {
        runtime.integrate(
            model,
            initial_state,
            input,
            input_model,
            normalized_duration,
            time_scale,
        )
    }
}
