use crate::integrator::private::IntegrationCandidate;
use crate::model::ContinuousModel;
use crate::scalar::{from_f64, from_usize};
use crate::{RungeKutta4, Scalar, UpdateError};

use super::{InputSegment, SolverDiagnostics};

#[derive(Clone, Copy, Debug)]
pub struct RungeKutta4Runtime<T> {
    pub(crate) policy: RungeKutta4<T>,
}

impl<T: Scalar> RungeKutta4Runtime<T> {
    pub(crate) const fn new(policy: RungeKutta4<T>) -> Self {
        Self { policy }
    }

    pub(crate) fn integrate<M: ContinuousModel<T, N>, const N: usize>(
        &self,
        model: &M,
        initial_state: &[T; N],
        input: InputSegment<T>,
        normalized_duration: T,
        time_scale: T,
    ) -> Result<IntegrationCandidate<T, N, Self>, UpdateError> {
        if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let configured_step = time_scale * self.policy.max_step_seconds;
        if !configured_step.is_finite() {
            return Err(UpdateError::InvalidDeltaTime);
        }
        let max_step = model.max_normalized_step().min(configured_step);
        if max_step <= T::zero() {
            return Err(UpdateError::StepSizeUnderflow);
        }

        let steps = required_step_count(normalized_duration, max_step, self.policy.max_steps)?;
        let step_count = from_usize::<T>(steps);

        let mut diagnostics = SolverDiagnostics::<T>::new();
        let mut state = *initial_state;

        for index in 0..steps {
            let start = normalized_duration * from_usize::<T>(index) / step_count;
            let end = if index + 1 == steps {
                normalized_duration
            } else {
                normalized_duration * from_usize::<T>(index + 1) / step_count
            };
            let step = end - start;
            if step <= T::zero() || start + step == start {
                return Err(UpdateError::StepSizeUnderflow);
            }

            state = step_rk4(model, &state, input, start, step, normalized_duration);
            diagnostics.derivative_evaluations =
                diagnostics.derivative_evaluations.saturating_add(4);
            if !state.iter().all(|value| value.is_finite()) {
                return Err(UpdateError::NonFiniteState);
            }

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
        }

        Ok(IntegrationCandidate {
            state,
            runtime: *self,
            diagnostics,
        })
    }
}

fn required_step_count<T: Scalar>(
    duration: T,
    max_step: T,
    max_steps: usize,
) -> Result<usize, UpdateError> {
    if duration <= max_step {
        return Ok(1);
    }

    let ratio = duration / max_step;
    if !ratio.is_finite() {
        return Err(UpdateError::StepBudgetExceeded);
    }
    let lower = ratio.floor();
    // Scaling physical time into normalized time and dividing it back can
    // leave an exact intended multiple a few ulps above its integer. Accept a
    // deliberately narrow allowance; a materially larger interval still
    // requires the next step.
    let allowance = from_f64::<T>(8.0) * T::epsilon() * ratio.max(T::one());
    let required = if ratio - lower <= allowance {
        lower
    } else {
        ratio.ceil()
    }
    .max(T::one());

    if required > from_usize::<T>(max_steps) {
        return Err(UpdateError::StepBudgetExceeded);
    }
    required.to_usize().ok_or(UpdateError::StepBudgetExceeded)
}

fn step_rk4<T: Scalar, M: ContinuousModel<T, N>, const N: usize>(
    model: &M,
    state: &[T; N],
    input: InputSegment<T>,
    position: T,
    step: T,
    total: T,
) -> [T; N] {
    let two = T::one() + T::one();
    let half = T::one() / two;
    let sixth = T::one() / (two + two + two);
    let start_fraction = position / total;
    let half_fraction = (position + step * half) / total;
    let end_fraction = if step == total - position {
        T::one()
    } else {
        (position + step) / total
    };

    let mut k1 = [T::zero(); N];
    let mut k2 = [T::zero(); N];
    let mut k3 = [T::zero(); N];
    let mut k4 = [T::zero(); N];
    let mut temporary = [T::zero(); N];

    model.derivative(state, input.value_at(start_fraction), &mut k1);
    for index in 0..N {
        temporary[index] = state[index] + step * half * k1[index];
    }
    model.derivative(&temporary, input.value_at(half_fraction), &mut k2);

    for index in 0..N {
        temporary[index] = state[index] + step * half * k2[index];
    }
    model.derivative(&temporary, input.value_at(half_fraction), &mut k3);

    for index in 0..N {
        temporary[index] = state[index] + step * k3[index];
    }
    model.derivative(&temporary, input.value_at(end_fraction), &mut k4);

    let mut candidate = [T::zero(); N];
    for index in 0..N {
        candidate[index] = state[index]
            + step * sixth * (k1[index] + two * k2[index] + two * k3[index] + k4[index]);
    }
    candidate
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;

    #[derive(Clone, Copy, Debug)]
    struct Accumulator;

    impl ContinuousModel<f64, 1> for Accumulator {
        fn derivative(&self, _state: &[f64; 1], input: f64, derivative: &mut [f64; 1]) {
            derivative[0] = input;
        }

        fn output(&self, state: &[f64; 1], _input: f64) -> f64 {
            state[0]
        }

        fn equilibrium(&self, input: f64) -> [f64; 1] {
            [input]
        }

        fn max_normalized_step(&self) -> f64 {
            1.0
        }
    }

    impl ContinuousModel<f32, 1> for Accumulator {
        fn derivative(&self, _state: &[f32; 1], input: f32, derivative: &mut [f32; 1]) {
            derivative[0] = input;
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
    fn linear_input_uses_the_original_complete_interval() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 0.25,
            max_steps: 4,
        });
        let outcome = runtime
            .integrate(
                &Accumulator,
                &[0.0],
                InputSegment::new(2.0, 6.0, crate::InputModel::Linear),
                1.0,
                1.0,
            )
            .unwrap();

        assert_relative_eq!(outcome.state[0], 4.0, epsilon = 1.0e-15);
        assert_eq!(outcome.diagnostics.accepted_steps, 4);
        assert_eq!(outcome.diagnostics.rejected_steps, 0);
        assert_eq!(outcome.diagnostics.derivative_evaluations, 16);
        assert_eq!(outcome.diagnostics.smallest_accepted_step, Some(0.25));
        assert_eq!(outcome.diagnostics.largest_accepted_step, Some(0.25));
    }

    #[test]
    fn insufficient_budget_rejects_the_complete_candidate() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 0.25,
            max_steps: 3,
        });
        let result = runtime.integrate(
            &Accumulator,
            &[0.0],
            InputSegment::new(2.0, 6.0, crate::InputModel::Linear),
            1.0,
            1.0,
        );

        assert!(matches!(result, Err(UpdateError::StepBudgetExceeded)));
    }

    #[test]
    fn exact_cap_multiple_does_not_require_a_rounding_residual_step() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 0.001,
            max_steps: 10,
        });
        let scale = core::f64::consts::TAU;
        let outcome = runtime
            .integrate(
                &Accumulator,
                &[0.0],
                InputSegment::new(2.0, 6.0, crate::InputModel::Linear),
                scale * 0.01,
                scale,
            )
            .unwrap();

        assert_eq!(outcome.diagnostics.accepted_steps, 10);
        assert_eq!(outcome.diagnostics.derivative_evaluations, 40);
        assert_relative_eq!(outcome.state[0], scale * 0.04, epsilon = 1.0e-15);

        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 0.001_f32,
            max_steps: 10,
        });
        let scale = core::f32::consts::TAU;
        let outcome = runtime
            .integrate(
                &Accumulator,
                &[0.0_f32],
                InputSegment::new(2.0, 6.0, crate::InputModel::Linear),
                scale * 0.01,
                scale,
            )
            .unwrap();

        assert_eq!(outcome.diagnostics.accepted_steps, 10);
        assert_eq!(outcome.diagnostics.derivative_evaluations, 40);
        assert_relative_eq!(outcome.state[0], scale * 0.04, epsilon = 5.0e-8);
    }

    #[test]
    fn scaled_cap_multiple_uses_exact_budget_but_real_excess_does_not() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 0.001,
            max_steps: 3,
        });
        let scale = core::f64::consts::TAU * 100.0;
        let exact = runtime
            .integrate(
                &Accumulator,
                &[0.0],
                InputSegment::new(0.0, 1.0, crate::InputModel::Linear),
                scale * 0.003,
                scale,
            )
            .unwrap();
        assert_eq!(exact.diagnostics.accepted_steps, 3);

        let over = runtime.integrate(
            &Accumulator,
            &[0.0],
            InputSegment::new(0.0, 1.0, crate::InputModel::Linear),
            scale * 0.003_01,
            scale,
        );
        assert!(matches!(over, Err(UpdateError::StepBudgetExceeded)));
    }

    #[test]
    fn positive_duration_that_underflows_in_the_ratio_still_takes_one_step() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: 10.0,
            max_steps: 1,
        });
        let outcome = runtime
            .integrate(
                &Accumulator,
                &[0.0],
                InputSegment::new(1.0, 1.0, crate::InputModel::CurrentHold),
                f64::from_bits(1),
                1.0,
            )
            .unwrap();

        assert_eq!(outcome.diagnostics.accepted_steps, 1);
        assert_eq!(outcome.diagnostics.derivative_evaluations, 4);
    }

    #[test]
    fn configured_step_underflow_is_reported() {
        let runtime = RungeKutta4Runtime::new(RungeKutta4 {
            max_step_seconds: f64::MIN_POSITIVE,
            max_steps: 1,
        });
        let result = runtime.integrate(
            &Accumulator,
            &[0.0],
            InputSegment::new(0.0, 1.0, crate::InputModel::Linear),
            1.0,
            f64::MIN_POSITIVE,
        );

        assert!(matches!(result, Err(UpdateError::StepSizeUnderflow)));
    }
}
