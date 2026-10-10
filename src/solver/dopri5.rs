use crate::model::ContinuousModel;
use crate::scalar::from_f64;
use crate::state::StateVector;
use crate::{IntegrationConfig, Scalar, UpdateError};

use super::InputSegment;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SolverDiagnostics<T> {
    pub(crate) accepted_steps: usize,
    pub(crate) rejected_steps: usize,
    pub(crate) derivative_evaluations: usize,
    pub(crate) smallest_accepted_step: Option<T>,
    pub(crate) largest_accepted_step: Option<T>,
}

impl<T> SolverDiagnostics<T> {
    const fn new() -> Self {
        Self {
            accepted_steps: 0,
            rejected_steps: 0,
            derivative_evaluations: 0,
            smallest_accepted_step: None,
            largest_accepted_step: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IntegrationOutcome<T, S> {
    pub(crate) state: S,
    pub(crate) diagnostics: SolverDiagnostics<T>,
}

pub(crate) fn integrate<T: Scalar, M, S>(
    model: &M,
    initial_state: &S,
    input: InputSegment<T>,
    normalized_duration: T,
    max_normalized_step: T,
    config: IntegrationConfig<T>,
) -> Result<IntegrationOutcome<T, S>, UpdateError>
where
    M: ContinuousModel<T, S>,
    S: StateVector<T>,
{
    if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
        return Err(UpdateError::InvalidDeltaTime);
    }

    let mut diagnostics = SolverDiagnostics::new();
    let mut state = *initial_state;
    let mut position = T::zero();
    let mut step = normalized_duration.min(max_normalized_step);

    for _ in 0..config.max_step_attempts {
        let remaining = normalized_duration - position;
        if remaining <= T::zero() {
            return Ok(IntegrationOutcome { state, diagnostics });
        }

        step = step.min(remaining).min(max_normalized_step);
        if step <= T::zero() || position + step == position {
            return Err(UpdateError::StepSizeUnderflow);
        }

        let (candidate, error) =
            dormand_prince_step(model, &state, input, position, step, normalized_duration);
        diagnostics.derivative_evaluations = diagnostics.derivative_evaluations.saturating_add(7);

        let error_norm = scaled_error(&state, &candidate, &error, config);
        if error_norm.is_finite() && error_norm <= T::one() && candidate.all_finite() {
            diagnostics.accepted_steps = diagnostics.accepted_steps.saturating_add(1);
            diagnostics.smallest_accepted_step = Some(
                diagnostics
                    .smallest_accepted_step
                    .map_or(step, |smallest| smallest.min(step)),
            );
            diagnostics.largest_accepted_step = Some(
                diagnostics
                    .largest_accepted_step
                    .map_or(step, |largest| largest.max(step)),
            );
            state = candidate;
            if step == remaining {
                return Ok(IntegrationOutcome { state, diagnostics });
            }
            position = position + step;
            step = (step * accepted_factor(error_norm)).min(max_normalized_step);
        } else {
            diagnostics.rejected_steps = diagnostics.rejected_steps.saturating_add(1);
            let factor = if error_norm.is_finite() {
                rejected_factor(error_norm)
            } else {
                from_f64(0.1)
            };
            step = step * factor;
            if !step.is_finite() {
                return Err(UpdateError::NonFiniteState);
            }
        }
    }

    Err(UpdateError::StepBudgetExceeded)
}

#[allow(clippy::too_many_lines)]
fn dormand_prince_step<T: Scalar, M, S>(
    model: &M,
    state: &S,
    input: InputSegment<T>,
    position: T,
    step: T,
    total: T,
) -> (S, S)
where
    M: ContinuousModel<T, S>,
    S: StateVector<T>,
{
    let mut k1 = S::zeros();
    let mut k2 = S::zeros();
    let mut k3 = S::zeros();
    let mut k4 = S::zeros();
    let mut k5 = S::zeros();
    let mut k6 = S::zeros();
    let mut k7 = S::zeros();
    let mut temporary = S::zeros();

    model.derivative(state, input.value_at(position / total), &mut k1);

    let c2 = fraction::<T>(1.0, 5.0);
    for index in 0..S::LEN {
        temporary.set_component(
            index,
            state.component(index) + step * c2 * k1.component(index),
        );
    }
    model.derivative(
        &temporary,
        input.value_at((position + c2 * step) / total),
        &mut k2,
    );

    let c3 = fraction::<T>(3.0, 10.0);
    for index in 0..S::LEN {
        temporary.set_component(
            index,
            state.component(index)
                + step
                    * (fraction::<T>(3.0, 40.0) * k1.component(index)
                        + fraction::<T>(9.0, 40.0) * k2.component(index)),
        );
    }
    model.derivative(
        &temporary,
        input.value_at((position + c3 * step) / total),
        &mut k3,
    );

    let c4 = fraction::<T>(4.0, 5.0);
    for index in 0..S::LEN {
        temporary.set_component(
            index,
            state.component(index)
                + step
                    * (fraction::<T>(44.0, 45.0) * k1.component(index)
                        - fraction::<T>(56.0, 15.0) * k2.component(index)
                        + fraction::<T>(32.0, 9.0) * k3.component(index)),
        );
    }
    model.derivative(
        &temporary,
        input.value_at((position + c4 * step) / total),
        &mut k4,
    );

    let c5 = fraction::<T>(8.0, 9.0);
    for index in 0..S::LEN {
        temporary.set_component(
            index,
            state.component(index)
                + step
                    * (fraction::<T>(19_372.0, 6_561.0) * k1.component(index)
                        - fraction::<T>(25_360.0, 2_187.0) * k2.component(index)
                        + fraction::<T>(64_448.0, 6_561.0) * k3.component(index)
                        - fraction::<T>(212.0, 729.0) * k4.component(index)),
        );
    }
    model.derivative(
        &temporary,
        input.value_at((position + c5 * step) / total),
        &mut k5,
    );

    for index in 0..S::LEN {
        temporary.set_component(
            index,
            state.component(index)
                + step
                    * (fraction::<T>(9_017.0, 3_168.0) * k1.component(index)
                        - fraction::<T>(355.0, 33.0) * k2.component(index)
                        + fraction::<T>(46_732.0, 5_247.0) * k3.component(index)
                        + fraction::<T>(49.0, 176.0) * k4.component(index)
                        - fraction::<T>(5_103.0, 18_656.0) * k5.component(index)),
        );
    }
    model.derivative(
        &temporary,
        input.value_at((position + step) / total),
        &mut k6,
    );

    let mut fifth_order = S::zeros();
    for index in 0..S::LEN {
        fifth_order.set_component(
            index,
            state.component(index)
                + step
                    * (fraction::<T>(35.0, 384.0) * k1.component(index)
                        + fraction::<T>(500.0, 1_113.0) * k3.component(index)
                        + fraction::<T>(125.0, 192.0) * k4.component(index)
                        - fraction::<T>(2_187.0, 6_784.0) * k5.component(index)
                        + fraction::<T>(11.0, 84.0) * k6.component(index)),
        );
    }
    model.derivative(
        &fifth_order,
        input.value_at((position + step) / total),
        &mut k7,
    );

    let mut error = S::zeros();
    for index in 0..S::LEN {
        let fourth_order = state.component(index)
            + step
                * (fraction::<T>(5_179.0, 57_600.0) * k1.component(index)
                    + fraction::<T>(7_571.0, 16_695.0) * k3.component(index)
                    + fraction::<T>(393.0, 640.0) * k4.component(index)
                    - fraction::<T>(92_097.0, 339_200.0) * k5.component(index)
                    + fraction::<T>(187.0, 2_100.0) * k6.component(index)
                    + fraction::<T>(1.0, 40.0) * k7.component(index));
        error.set_component(index, fifth_order.component(index) - fourth_order);
    }

    (fifth_order, error)
}

fn scaled_error<T: Scalar, S: StateVector<T>>(
    state: &S,
    candidate: &S,
    error: &S,
    config: IntegrationConfig<T>,
) -> T {
    let mut norm = T::zero();
    for index in 0..S::LEN {
        let scale = config.tolerances.absolute
            + config.tolerances.relative
                * state
                    .component(index)
                    .abs()
                    .max(candidate.component(index).abs());
        let component = error.component(index).abs() / scale;
        if component > norm {
            norm = component;
        }
    }
    norm
}

fn accepted_factor<T: Scalar>(error_norm: T) -> T {
    if error_norm == T::zero() {
        return from_f64(5.0);
    }
    let raw = from_f64::<T>(0.9) * error_norm.powf(from_f64(-0.2));
    raw.max(from_f64(0.2)).min(from_f64(5.0))
}

fn rejected_factor<T: Scalar>(error_norm: T) -> T {
    let raw = from_f64::<T>(0.9) * error_norm.powf(from_f64(-0.2));
    raw.max(from_f64(0.1)).min(from_f64(0.5))
}

fn fraction<T: Scalar>(numerator: f64, denominator: f64) -> T {
    from_f64::<T>(numerator) / from_f64::<T>(denominator)
}

#[cfg(test)]
pub(crate) fn integrate_euler<T: Scalar, M, S>(
    model: &M,
    initial_state: &S,
    input: InputSegment<T>,
    normalized_duration: T,
    steps: usize,
) -> S
where
    M: ContinuousModel<T, S>,
    S: StateVector<T>,
{
    let mut state = *initial_state;
    let step = normalized_duration / crate::scalar::from_usize::<T>(steps);
    let mut derivative = S::zeros();
    for index in 0..steps {
        let position = crate::scalar::from_usize::<T>(index) * step;
        model.derivative(
            &state,
            input.value_at(position / normalized_duration),
            &mut derivative,
        );
        for component in 0..S::LEN {
            state.set_component(
                component,
                state.component(component) + step * derivative.component(component),
            );
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    use crate::InputModel;
    use crate::model::RepeatedPole;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct SplitState {
        source: [f64; 1],
        auxiliary: [f64; 1],
    }

    impl StateVector<f64> for SplitState {
        const LEN: usize = 2;

        fn zeros() -> Self {
            Self {
                source: [0.0],
                auxiliary: [0.0],
            }
        }

        fn component(&self, index: usize) -> f64 {
            match index {
                0 => self.source[0],
                1 => self.auxiliary[0],
                _ => panic!("split-state component index out of bounds"),
            }
        }

        fn set_component(&mut self, index: usize, value: f64) {
            match index {
                0 => self.source[0] = value,
                1 => self.auxiliary[0] = value,
                _ => panic!("split-state component index out of bounds"),
            }
        }
    }

    struct TwoStageModel;

    impl ContinuousModel<f64, [f64; 2]> for TwoStageModel {
        fn derivative(&self, state: &[f64; 2], input: f64, derivative: &mut [f64; 2]) {
            derivative[0] = input - state[0];
            derivative[1] = state[0] - 2.0 * state[1];
        }

        fn output(&self, state: &[f64; 2], _input: f64) -> f64 {
            state[1]
        }

        fn equilibrium(&self, input: f64) -> [f64; 2] {
            [input, input / 2.0]
        }

        fn max_normalized_step(&self) -> f64 {
            0.5
        }
    }

    impl ContinuousModel<f64, SplitState> for TwoStageModel {
        fn derivative(&self, state: &SplitState, input: f64, derivative: &mut SplitState) {
            derivative.source[0] = input - state.source[0];
            derivative.auxiliary[0] = state.source[0] - 2.0 * state.auxiliary[0];
        }

        fn output(&self, state: &SplitState, _input: f64) -> f64 {
            state.auxiliary[0]
        }

        fn equilibrium(&self, input: f64) -> SplitState {
            SplitState {
                source: [input],
                auxiliary: [input / 2.0],
            }
        }

        fn max_normalized_step(&self) -> f64 {
            0.5
        }
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exact equality is the behavioral-parity contract under test.
    fn split_fixed_state_is_bit_exact_with_equivalent_array_state() {
        let model = TwoStageModel;
        let input = InputSegment::new(-0.25, 1.75, InputModel::Linear);
        let array = integrate(
            &model,
            &[0.4, -0.1],
            input,
            1.25,
            0.5,
            IntegrationConfig::default(),
        )
        .unwrap();
        let split = integrate(
            &model,
            &SplitState {
                source: [0.4],
                auxiliary: [-0.1],
            },
            input,
            1.25,
            0.5,
            IntegrationConfig::default(),
        )
        .unwrap();

        assert_eq!(
            array.state,
            [split.state.source[0], split.state.auxiliary[0]]
        );
        assert_eq!(array.diagnostics, split.diagnostics);
    }

    #[test]
    fn adaptive_solver_matches_exact_first_order_step() {
        let model = RepeatedPole::<f64>::new(1);
        let rate = model.rate();
        let duration = 0.75;
        let actual = integrate(
            &model,
            &[0.0],
            InputSegment::new(0.0, 1.0, InputModel::CurrentHold),
            duration,
            1.0 / rate,
            IntegrationConfig::default(),
        )
        .unwrap()
        .state[0];
        let expected = 1.0 - (-rate * duration).exp();
        assert_relative_eq!(actual, expected, epsilon = 5.0e-9);
    }

    #[test]
    fn euler_helper_exhibits_first_order_convergence() {
        let model = RepeatedPole::<f64>::new(1);
        let duration = 0.5;
        let input = InputSegment::new(0.0, 1.0, InputModel::CurrentHold);
        let exact = 1.0 - (-model.rate() * duration).exp();
        let coarse = integrate_euler(&model, &[0.0], input, duration, 20)[0];
        let fine = integrate_euler(&model, &[0.0], input, duration, 40)[0];
        let coarse_error = (coarse - exact).abs();
        let fine_error = (fine - exact).abs();
        assert!(fine_error < coarse_error);
        assert!((coarse_error / fine_error - 2.0).abs() < 0.15);
    }

    #[test]
    fn work_budget_does_not_discard_unintegrated_time() {
        let model = RepeatedPole::<f64>::new(1);
        let config = IntegrationConfig {
            max_step_attempts: 1,
            ..IntegrationConfig::default()
        };
        let result = integrate(
            &model,
            &[0.0],
            InputSegment::new(0.0, 1.0, InputModel::CurrentHold),
            2.0,
            0.1,
            config,
        );
        assert_eq!(result, Err(UpdateError::StepBudgetExceeded));
    }
}
