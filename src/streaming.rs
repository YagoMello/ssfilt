use crate::model::ContinuousModel;
use crate::solver::{InputSegment, integrate};
use crate::{
    InputModel, IntegrationConfig, IntegrationDiagnostics, ResetError, Scalar, UpdateError,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamingCore<T, M, const N: usize> {
    pub(crate) model: M,
    pub(crate) state: [T; N],
    pub(crate) previous_input: T,
    pub(crate) output: T,
    time_scale: T,
    input_model: InputModel,
    integration: IntegrationConfig<T>,
    pub(crate) last_diagnostics: IntegrationDiagnostics<T>,
    pub(crate) at_equilibrium: bool,
}

impl<T: Scalar, M: ContinuousModel<T, N>, const N: usize> StreamingCore<T, M, N> {
    pub(crate) fn new(
        model: M,
        initial_input: T,
        time_scale: T,
        input_model: InputModel,
        integration: IntegrationConfig<T>,
    ) -> Self {
        let state = model.equilibrium(initial_input);
        let output = model.output(&state, initial_input);
        Self {
            model,
            state,
            previous_input: initial_input,
            output,
            time_scale,
            input_model,
            integration,
            last_diagnostics: IntegrationDiagnostics::empty(),
            at_equilibrium: true,
        }
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

        let mut max_normalized_step = self.model.max_normalized_step();
        if let Some(max_step_seconds) = self.integration.max_step_seconds {
            let configured = self.time_scale * max_step_seconds;
            if !configured.is_finite() {
                return Err(UpdateError::InvalidDeltaTime);
            }
            max_normalized_step = max_normalized_step.min(configured);
        }

        let segment = InputSegment::new(self.previous_input, input, self.input_model);
        let outcome = integrate(
            &self.model,
            &self.state,
            segment,
            normalized_duration,
            max_normalized_step,
            self.integration,
        )?;
        let next_output = self.model.output(&outcome.state, input);
        if !next_output.is_finite() {
            return Err(UpdateError::NonFiniteState);
        }

        self.state = outcome.state;
        self.previous_input = input;
        self.output = next_output;
        self.last_diagnostics =
            IntegrationDiagnostics::from_solver(outcome.diagnostics, self.time_scale);
        self.at_equilibrium = false;
        Ok(next_output)
    }

    pub(crate) fn reset(&mut self) {
        self.state = self.model.equilibrium(T::zero());
        self.previous_input = T::zero();
        self.output = self.model.output(&self.state, T::zero());
        self.last_diagnostics = IntegrationDiagnostics::empty();
        self.at_equilibrium = true;
    }

    pub(crate) fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        if !input.is_finite() {
            return Err(ResetError::NonFiniteInput);
        }
        self.state = self.model.equilibrium(input);
        self.previous_input = input;
        self.output = self.model.output(&self.state, input);
        self.last_diagnostics = IntegrationDiagnostics::empty();
        self.at_equilibrium = true;
        Ok(())
    }
}
