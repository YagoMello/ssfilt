use crate::model::ContinuousModel;
use crate::scalar::from_f64;
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
pub(crate) struct IntegrationOutcome<T, const N: usize> {
    pub(crate) state: [T; N],
    pub(crate) endpoint_derivative: [T; N],
    pub(crate) diagnostics: SolverDiagnostics<T>,
}

pub(crate) fn integrate<T: Scalar, M, const N: usize>(
    model: &M,
    initial_state: &[T; N],
    input: InputSegment<T>,
    normalized_duration: T,
    max_normalized_step: T,
    config: IntegrationConfig<T>,
    initial_derivative: Option<[T; N]>,
) -> Result<IntegrationOutcome<T, N>, UpdateError>
where
    M: ContinuousModel<T, N>,
{
    if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
        return Err(UpdateError::InvalidDeltaTime);
    }

    let mut diagnostics = SolverDiagnostics::new();
    let mut state = *initial_state;
    let mut position = T::zero();
    let mut input_fraction = T::zero();
    let mut step = normalized_duration.min(max_normalized_step);
    let mut start_derivative = initial_derivative;

    for _ in 0..config.max_step_attempts {
        let remaining = normalized_duration - position;
        if remaining <= T::zero() {
            return Ok(IntegrationOutcome {
                state,
                endpoint_derivative: start_derivative
                    .expect("a completed interval has an endpoint derivative"),
                diagnostics,
            });
        }

        step = step.min(remaining).min(max_normalized_step);
        if step <= T::zero() || position + step == position {
            return Err(UpdateError::StepSizeUnderflow);
        }

        let outcome = dormand_prince_step(
            model,
            &state,
            input,
            position,
            step,
            normalized_duration,
            input_fraction,
            start_derivative.as_ref(),
        );
        diagnostics.derivative_evaluations = diagnostics
            .derivative_evaluations
            .saturating_add(if start_derivative.is_some() { 6 } else { 7 });

        let error_norm = scaled_error(&state, &outcome.candidate, &outcome.error, config);
        if error_norm.is_finite() && error_norm <= T::one() && all_finite(&outcome.candidate) {
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
            state = outcome.candidate;
            if step == remaining {
                return Ok(IntegrationOutcome {
                    state,
                    endpoint_derivative: outcome.end_derivative,
                    diagnostics,
                });
            }
            position = position + step;
            input_fraction = outcome.end_fraction;
            start_derivative = Some(outcome.end_derivative);
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
            start_derivative = Some(outcome.start_derivative);
        }
    }

    Err(UpdateError::StepBudgetExceeded)
}

struct StepOutcome<T, const N: usize> {
    candidate: [T; N],
    error: [T; N],
    start_derivative: [T; N],
    end_derivative: [T; N],
    end_fraction: T,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn dormand_prince_step<T: Scalar, M, const N: usize>(
    model: &M,
    state: &[T; N],
    input: InputSegment<T>,
    position: T,
    step: T,
    total: T,
    start_fraction: T,
    start_derivative: Option<&[T; N]>,
) -> StepOutcome<T, N>
where
    M: ContinuousModel<T, N>,
{
    let mut k1 = [T::zero(); N];
    let mut k2 = [T::zero(); N];
    let mut k3 = [T::zero(); N];
    let mut k4 = [T::zero(); N];
    let mut k5 = [T::zero(); N];
    let mut k6 = [T::zero(); N];
    let mut k7 = [T::zero(); N];
    let mut temporary = [T::zero(); N];
    let step_fraction = step / total;
    let end_fraction = if step == total - position {
        T::one()
    } else {
        (position + step) / total
    };

    if let Some(start_derivative) = start_derivative {
        k1 = *start_derivative;
    } else {
        model.derivative(state, input.value_at(start_fraction), &mut k1);
    }

    let c2 = fraction::<T>(1.0, 5.0);
    for index in 0..N {
        temporary[index] = state[index] + step * c2 * k1[index];
    }
    model.derivative(
        &temporary,
        input.value_at(start_fraction + c2 * step_fraction),
        &mut k2,
    );

    let c3 = fraction::<T>(3.0, 10.0);
    for index in 0..N {
        temporary[index] = state[index]
            + step * (fraction::<T>(3.0, 40.0) * k1[index] + fraction::<T>(9.0, 40.0) * k2[index]);
    }
    model.derivative(
        &temporary,
        input.value_at(start_fraction + c3 * step_fraction),
        &mut k3,
    );

    let c4 = fraction::<T>(4.0, 5.0);
    for index in 0..N {
        temporary[index] = state[index]
            + step
                * (fraction::<T>(44.0, 45.0) * k1[index] - fraction::<T>(56.0, 15.0) * k2[index]
                    + fraction::<T>(32.0, 9.0) * k3[index]);
    }
    model.derivative(
        &temporary,
        input.value_at(start_fraction + c4 * step_fraction),
        &mut k4,
    );

    let c5 = fraction::<T>(8.0, 9.0);
    for index in 0..N {
        temporary[index] = state[index]
            + step
                * (fraction::<T>(19_372.0, 6_561.0) * k1[index]
                    - fraction::<T>(25_360.0, 2_187.0) * k2[index]
                    + fraction::<T>(64_448.0, 6_561.0) * k3[index]
                    - fraction::<T>(212.0, 729.0) * k4[index]);
    }
    model.derivative(
        &temporary,
        input.value_at(start_fraction + c5 * step_fraction),
        &mut k5,
    );

    for index in 0..N {
        temporary[index] = state[index]
            + step
                * (fraction::<T>(9_017.0, 3_168.0) * k1[index]
                    - fraction::<T>(355.0, 33.0) * k2[index]
                    + fraction::<T>(46_732.0, 5_247.0) * k3[index]
                    + fraction::<T>(49.0, 176.0) * k4[index]
                    - fraction::<T>(5_103.0, 18_656.0) * k5[index]);
    }
    model.derivative(&temporary, input.value_at(end_fraction), &mut k6);

    let mut fifth_order = [T::zero(); N];
    for index in 0..N {
        fifth_order[index] = state[index]
            + step
                * (fraction::<T>(35.0, 384.0) * k1[index]
                    + fraction::<T>(500.0, 1_113.0) * k3[index]
                    + fraction::<T>(125.0, 192.0) * k4[index]
                    - fraction::<T>(2_187.0, 6_784.0) * k5[index]
                    + fraction::<T>(11.0, 84.0) * k6[index]);
    }
    model.derivative(&fifth_order, input.value_at(end_fraction), &mut k7);

    let mut error = [T::zero(); N];
    for index in 0..N {
        let fourth_order = state[index]
            + step
                * (fraction::<T>(5_179.0, 57_600.0) * k1[index]
                    + fraction::<T>(7_571.0, 16_695.0) * k3[index]
                    + fraction::<T>(393.0, 640.0) * k4[index]
                    - fraction::<T>(92_097.0, 339_200.0) * k5[index]
                    + fraction::<T>(187.0, 2_100.0) * k6[index]
                    + fraction::<T>(1.0, 40.0) * k7[index]);
        error[index] = fifth_order[index] - fourth_order;
    }

    StepOutcome {
        candidate: fifth_order,
        error,
        start_derivative: k1,
        end_derivative: k7,
        end_fraction,
    }
}

fn scaled_error<T: Scalar, const N: usize>(
    state: &[T; N],
    candidate: &[T; N],
    error: &[T; N],
    config: IntegrationConfig<T>,
) -> T {
    let mut norm = T::zero();
    for index in 0..N {
        let scale = config.tolerances.absolute
            + config.tolerances.relative * state[index].abs().max(candidate[index].abs());
        let component = error[index].abs() / scale;
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

fn all_finite<T: Scalar, const N: usize>(values: &[T; N]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn fraction<T: Scalar>(numerator: f64, denominator: f64) -> T {
    from_f64::<T>(numerator) / from_f64::<T>(denominator)
}

#[cfg(test)]
pub(crate) fn integrate_euler<T: Scalar, M, const N: usize>(
    model: &M,
    initial_state: &[T; N],
    input: InputSegment<T>,
    normalized_duration: T,
    steps: usize,
) -> [T; N]
where
    M: ContinuousModel<T, N>,
{
    let mut state = *initial_state;
    let step = normalized_duration / crate::scalar::from_usize::<T>(steps);
    let mut derivative = [T::zero(); N];
    for index in 0..steps {
        let position = crate::scalar::from_usize::<T>(index) * step;
        model.derivative(
            &state,
            input.value_at(position / normalized_duration),
            &mut derivative,
        );
        for component in 0..N {
            state[component] = state[component] + step * derivative[component];
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::vec::Vec;

    use approx::assert_relative_eq;

    use super::*;
    use crate::InputModel;
    use crate::model::RepeatedPole;

    struct RecordingModel {
        inputs: RefCell<Vec<f32>>,
    }

    impl ContinuousModel<f32, 1> for RecordingModel {
        fn derivative(&self, _state: &[f32; 1], input: f32, derivative: &mut [f32; 1]) {
            self.inputs.borrow_mut().push(input);
            derivative[0] = 0.0;
        }

        fn output(&self, state: &[f32; 1], _input: f32) -> f32 {
            state[0]
        }

        fn equilibrium(&self, input: f32) -> [f32; 1] {
            [input]
        }

        fn max_normalized_step(&self) -> f32 {
            1.0
        }
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
            None,
        )
        .unwrap()
        .state[0];
        let expected = 1.0 - (-rate * duration).exp();
        assert_relative_eq!(actual, expected, epsilon = 5.0e-9);
    }

    #[test]
    fn cached_linear_endpoint_matches_fresh_derivative_evaluation() {
        let model = RepeatedPole::<f64>::new(1);
        let config = IntegrationConfig::default();
        let first = integrate(
            &model,
            &[0.0],
            InputSegment::new(0.0, 1.0, InputModel::Linear),
            0.125,
            1.0,
            config,
            None,
        )
        .unwrap();
        let input = InputSegment::new(1.0, -0.25, InputModel::Linear);
        let cached = integrate(
            &model,
            &first.state,
            input,
            0.13,
            1.0,
            config,
            Some(first.endpoint_derivative),
        )
        .unwrap();
        let uncached = integrate(&model, &first.state, input, 0.13, 1.0, config, None).unwrap();

        assert_eq!(cached.state, uncached.state);
        assert_eq!(
            cached.diagnostics.accepted_steps,
            uncached.diagnostics.accepted_steps
        );
        assert_eq!(
            cached.diagnostics.rejected_steps,
            uncached.diagnostics.rejected_steps
        );
        assert_eq!(
            cached.diagnostics.derivative_evaluations + 1,
            uncached.diagnostics.derivative_evaluations
        );
    }

    #[test]
    fn rejected_attempt_reuses_the_unchanged_start_derivative() {
        let model = RepeatedPole::<f64>::new(1);
        let config = IntegrationConfig {
            tolerances: crate::Tolerances::new(1.0e-14, 1.0e-14),
            ..IntegrationConfig::default()
        };
        let outcome = integrate(
            &model,
            &[0.0],
            InputSegment::new(0.0, 1.0, InputModel::CurrentHold),
            1.0,
            1.0,
            config,
            None,
        )
        .unwrap();
        let attempts = outcome.diagnostics.accepted_steps + outcome.diagnostics.rejected_steps;
        assert!(outcome.diagnostics.rejected_steps > 0);
        assert_eq!(outcome.diagnostics.derivative_evaluations, 6 * attempts + 1);
    }

    #[test]
    fn final_stages_use_the_exact_input_endpoint() {
        let position = 4.136_500_4_f32 / 256.0;
        let step = 76.776_54_f32 / 256.0;
        let total = 80.913_04_f32 / 256.0;
        assert_eq!(step, total - position);
        assert!(position / total + step / total > 1.0);

        let model = RecordingModel {
            inputs: RefCell::new(Vec::new()),
        };
        let _ = dormand_prince_step(
            &model,
            &[0.0],
            InputSegment::new(0.0, 1.0, InputModel::Linear),
            position,
            step,
            total,
            position / total,
            None,
        );
        let inputs = model.inputs.borrow();
        assert!(inputs.iter().all(|input| *input <= 1.0));
        assert_eq!(&inputs[inputs.len() - 2..], &[1.0, 1.0]);
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
            None,
        );
        assert_eq!(result, Err(UpdateError::StepBudgetExceeded));
    }
}
