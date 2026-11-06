use crate::integrator::private::IntegratorImpl;
use crate::model::ContinuousModel;
use crate::solver::InputSegment;
use crate::{
    BuildError, InputModel, IntegrationDiagnostics, Integrator, ResetError, Scalar, UpdateError,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamingCore<T, M, const N: usize, I>
where
    T: Scalar,
    I: Integrator<T, N>,
{
    pub(crate) model: M,
    pub(crate) state: [T; N],
    pub(crate) previous_input: T,
    pub(crate) output: T,
    time_scale: T,
    input_model: InputModel,
    integration: <I as IntegratorImpl<T, N>>::Runtime,
    pub(crate) last_diagnostics: IntegrationDiagnostics<T>,
    pub(crate) at_equilibrium: bool,
}

impl<T, M, I, const N: usize> StreamingCore<T, M, N, I>
where
    T: Scalar,
    M: ContinuousModel<T, N>,
    I: Integrator<T, N>,
{
    pub(crate) fn new(
        model: M,
        initial_input: T,
        time_scale: T,
        input_model: InputModel,
        integrator: I,
    ) -> Result<Self, BuildError> {
        let state = model.equilibrium(initial_input);
        let output = model.output(&state, initial_input);
        if !state.iter().all(|value| value.is_finite()) || !output.is_finite() {
            return Err(BuildError::UnrepresentableInitialEquilibrium);
        }
        let integration = integrator.into_runtime(
            crate::integrator::private::Token,
            &model,
            &state,
            initial_input,
            input_model,
        );
        Ok(Self {
            model,
            state,
            previous_input: initial_input,
            output,
            time_scale,
            input_model,
            integration,
            last_diagnostics: IntegrationDiagnostics::empty(),
            at_equilibrium: true,
        })
    }

    pub(crate) fn update(&mut self, input: T, dt_seconds: T) -> Result<T, UpdateError> {
        if !input.is_finite() {
            return Err(UpdateError::NonFiniteInput);
        }
        if !dt_seconds.is_finite() || dt_seconds <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let normalized_duration = self.time_scale * dt_seconds;
        if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let previous_input = self.previous_input;
        let constant_at_equilibrium = self.at_equilibrium
            && match self.input_model {
                InputModel::Linear | InputModel::CurrentHold => input == previous_input,
                InputModel::PreviousHold => true,
            };
        if constant_at_equilibrium {
            let next_output = self.model.output(&self.state, input);
            if !next_output.is_finite() {
                return Err(UpdateError::NonFiniteState);
            }
            self.previous_input = input;
            self.output = next_output;
            self.last_diagnostics = IntegrationDiagnostics::equilibrium_shortcut();
            self.at_equilibrium = input == previous_input;
            return Ok(next_output);
        }

        let segment = InputSegment::new(self.previous_input, input, self.input_model);
        let outcome = I::integrate(
            crate::integrator::private::Token,
            &self.integration,
            &self.model,
            &self.state,
            segment,
            self.input_model,
            normalized_duration,
            self.time_scale,
        )?;
        let next_output = self.model.output(&outcome.state, input);
        if !next_output.is_finite() {
            return Err(UpdateError::NonFiniteState);
        }

        self.state = outcome.state;
        self.previous_input = input;
        self.output = next_output;
        self.integration = outcome.runtime;
        self.last_diagnostics =
            IntegrationDiagnostics::from_solver(outcome.diagnostics, self.time_scale);
        self.at_equilibrium = false;
        Ok(next_output)
    }

    pub(crate) fn reset(&mut self) {
        self.state = self.model.equilibrium(T::zero());
        self.previous_input = T::zero();
        self.output = self.model.output(&self.state, T::zero());
        self.integration = I::reset_runtime(
            crate::integrator::private::Token,
            &self.integration,
            &self.model,
            &self.state,
            T::zero(),
            self.input_model,
        );
        self.last_diagnostics = IntegrationDiagnostics::empty();
        self.at_equilibrium = true;
    }

    pub(crate) fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        if !input.is_finite() {
            return Err(ResetError::NonFiniteInput);
        }
        let state = self.model.equilibrium(input);
        let output = self.model.output(&state, input);
        if !state.iter().all(|value| value.is_finite()) || !output.is_finite() {
            return Err(ResetError::UnrepresentableSteadyState);
        }

        self.state = state;
        self.previous_input = input;
        self.output = output;
        self.integration = I::reset_runtime(
            crate::integrator::private::Token,
            &self.integration,
            &self.model,
            &self.state,
            input,
            self.input_model,
        );
        self.last_diagnostics = IntegrationDiagnostics::empty();
        self.at_equilibrium = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrator::private::{IntegrationCandidate, IntegratorImpl, Token};
    use crate::solver::SolverDiagnostics;

    #[derive(Clone, Copy, Debug)]
    struct InvalidEndpointModel;

    impl ContinuousModel<f64, 1> for InvalidEndpointModel {
        fn derivative(&self, _state: &[f64; 1], _input: f64, derivative: &mut [f64; 1]) {
            derivative[0] = 0.0;
        }

        fn output(&self, state: &[f64; 1], _input: f64) -> f64 {
            if state[0].to_bits() == 1.0f64.to_bits() {
                f64::NAN
            } else {
                state[0]
            }
        }

        fn equilibrium(&self, input: f64) -> [f64; 1] {
            [input]
        }

        fn max_normalized_step(&self) -> f64 {
            1.0
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct CandidatePolicy;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct CandidateRuntime {
        generation: usize,
    }

    impl IntegratorImpl<f64, 1> for CandidatePolicy {
        type Runtime = CandidateRuntime;

        fn is_valid(self, _token: Token) -> bool {
            true
        }

        fn into_runtime<M: ContinuousModel<f64, 1>>(
            self,
            _token: Token,
            _model: &M,
            _state: &[f64; 1],
            _input: f64,
            _input_model: InputModel,
        ) -> Self::Runtime {
            CandidateRuntime { generation: 0 }
        }

        fn reset_runtime<M: ContinuousModel<f64, 1>>(
            _token: Token,
            _runtime: &Self::Runtime,
            _model: &M,
            _state: &[f64; 1],
            _input: f64,
            _input_model: InputModel,
        ) -> Self::Runtime {
            CandidateRuntime { generation: 0 }
        }

        fn integrate<M: ContinuousModel<f64, 1>>(
            _token: Token,
            runtime: &Self::Runtime,
            _model: &M,
            _initial_state: &[f64; 1],
            _input: InputSegment<f64>,
            _input_model: InputModel,
            _normalized_duration: f64,
            _time_scale: f64,
        ) -> Result<IntegrationCandidate<f64, 1, Self::Runtime>, UpdateError> {
            Ok(IntegrationCandidate {
                state: [1.0],
                runtime: CandidateRuntime {
                    generation: runtime.generation + 1,
                },
                diagnostics: SolverDiagnostics::new(),
            })
        }
    }

    #[test]
    fn invalid_endpoint_output_discards_candidate_runtime() {
        let mut core = StreamingCore::new(
            InvalidEndpointModel,
            0.0,
            1.0,
            InputModel::CurrentHold,
            CandidatePolicy,
        )
        .unwrap();
        let before = core;

        assert_eq!(core.update(0.5, 0.1), Err(UpdateError::NonFiniteState));
        assert_eq!(core.state, before.state);
        assert_eq!(core.previous_input, before.previous_input);
        assert_eq!(core.output, before.output);
        assert_eq!(core.integration, before.integration);
        assert_eq!(core.last_diagnostics, before.last_diagnostics);
        assert_eq!(core.at_equilibrium, before.at_equilibrium);
    }
}
