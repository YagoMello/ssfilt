//! Private bounded trajectory-replay candidate for forward-backward experiments.
//!
//! This retains the complete forward state at every supplied knot, initializes
//! the reverse pass from the analytic constant-continuation tail, and replays
//! each forward interval inside the reverse adaptive solve. It is test-only
//! investigation code, not a selected implementation or public API.

use crate::batch_prototype::{
    MAX_INTERVAL_ATTEMPTS, PrototypeDiagnostics, PrototypeError, PrototypePass,
    maximum_normalized_step, validate_with_workspace,
};
use crate::batch_tail::analytic_tail_state;
use crate::model::{Butterworth, ContinuousModel};
use crate::solver::{InputSegment, integrate};
use crate::state::StateVector;
use crate::{InputModel, IntegrationConfig, Scalar};

#[derive(Clone, Copy, Debug, PartialEq)]
struct TrajectoryReplayState<T, const N: usize> {
    replay: [T; N],
    reverse: [T; N],
}

impl<T: Scalar, const N: usize> StateVector<T> for TrajectoryReplayState<T, N> {
    const LEN: usize = 2 * N;

    fn zeros() -> Self {
        Self {
            replay: [T::zero(); N],
            reverse: [T::zero(); N],
        }
    }

    fn component(&self, index: usize) -> T {
        if index < N {
            self.replay[index]
        } else {
            self.reverse[index - N]
        }
    }

    fn set_component(&mut self, index: usize, value: T) {
        if index < N {
            self.replay[index] = value;
        } else {
            self.reverse[index - N] = value;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TrajectoryReplayModel<T, const N: usize> {
    pass: Butterworth<T, N>,
}

impl<T: Scalar, const N: usize> TrajectoryReplayModel<T, N> {
    const fn new(pass: Butterworth<T, N>) -> Self {
        Self { pass }
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, TrajectoryReplayState<T, N>>
    for TrajectoryReplayModel<T, N>
{
    fn derivative(
        &self,
        state: &TrajectoryReplayState<T, N>,
        input: T,
        derivative: &mut TrajectoryReplayState<T, N>,
    ) {
        self.pass
            .derivative(&state.replay, input, &mut derivative.replay);
        for value in &mut derivative.replay {
            *value = -*value;
        }

        let forward_output = self.pass.output(&state.replay, input);
        self.pass
            .derivative(&state.reverse, forward_output, &mut derivative.reverse);
    }

    fn output(&self, state: &TrajectoryReplayState<T, N>, input: T) -> T {
        let forward_output = self.pass.output(&state.replay, input);
        self.pass.output(&state.reverse, forward_output)
    }

    fn equilibrium(&self, input: T) -> TrajectoryReplayState<T, N> {
        TrajectoryReplayState {
            replay: self.pass.equilibrium(input),
            reverse: self.pass.equilibrium(input),
        }
    }

    fn max_normalized_step(&self) -> T {
        self.pass.max_normalized_step()
    }
}

/// Required caller-owned scalar workspace for one trajectory-aware run.
pub(crate) fn trajectory_workspace_len<const N: usize>(
    record_length: usize,
) -> Result<usize, PrototypeError> {
    record_length
        .checked_mul(N)
        .and_then(|states| states.checked_add(record_length))
        .ok_or(PrototypeError::WorkspaceSizeOverflow)
}

pub(crate) fn trajectory_replay<const N: usize, T: Scalar>(
    cutoff_hz: T,
    intervals_seconds: &[T],
    samples: &[T],
    output: &mut [T],
    workspace: &mut [T],
) -> Result<PrototypeDiagnostics, PrototypeError>
where
    IntegrationConfig<T>: Default,
{
    let integration = IntegrationConfig {
        max_step_attempts: MAX_INTERVAL_ATTEMPTS,
        ..IntegrationConfig::default()
    };
    trajectory_replay_with_configs::<N, T>(
        cutoff_hz,
        intervals_seconds,
        samples,
        output,
        workspace,
        integration,
        integration,
    )
}

#[allow(clippy::too_many_arguments)]
fn trajectory_replay_with_configs<const N: usize, T: Scalar>(
    cutoff_hz: T,
    intervals_seconds: &[T],
    samples: &[T],
    output: &mut [T],
    workspace: &mut [T],
    forward_integration: IntegrationConfig<T>,
    reverse_integration: IntegrationConfig<T>,
) -> Result<PrototypeDiagnostics, PrototypeError> {
    let (record_length, time_scale) = validate_with_workspace::<N, T, _>(
        cutoff_hz,
        intervals_seconds,
        samples,
        output.len(),
        workspace.len(),
        forward_integration,
        |length| trajectory_workspace_len::<N>(length).ok(),
    )?;
    if !reverse_integration.is_valid() {
        return Err(PrototypeError::InvalidIntegrationConfig);
    }

    let model = Butterworth::<T, N>::new();
    let forward_max_step = maximum_normalized_step(&model, time_scale, forward_integration)?;
    let replay_model = TrajectoryReplayModel::new(model);
    let reverse_max_step = maximum_normalized_step(&model, time_scale, reverse_integration)?;
    let state_storage_length = record_length
        .checked_mul(N)
        .ok_or(PrototypeError::WorkspaceSizeOverflow)?;
    let required_workspace = trajectory_workspace_len::<N>(record_length)?;
    let (forward_states, remainder) =
        workspace[..required_workspace].split_at_mut(state_storage_length);
    let staged = &mut remainder[..record_length];

    let mut diagnostics = PrototypeDiagnostics::default();
    let mut forward_state = model.equilibrium(samples[0]);
    store_state(forward_states, 0, &forward_state);
    for interval_index in 0..intervals_seconds.len() {
        let normalized_duration = time_scale * intervals_seconds[interval_index];
        let segment = InputSegment::new(
            samples[interval_index],
            samples[interval_index + 1],
            InputModel::Linear,
        );
        let outcome = integrate(
            &model,
            &forward_state,
            segment,
            normalized_duration,
            forward_max_step,
            forward_integration,
        )
        .map_err(|source| PrototypeError::Integration {
            pass: PrototypePass::Forward,
            interval: interval_index,
            source,
        })?;
        forward_state = outcome.state;
        diagnostics
            .forward
            .add_with_derivative_scale(&outcome.diagnostics, 1)?;
        store_state(forward_states, interval_index + 1, &forward_state);
    }

    let last = record_length - 1;
    let mut reverse_state = analytic_tail_state(&model, &forward_state, samples[last])
        .ok_or(PrototypeError::TailInitialization)?;
    staged[last] = model.output(&reverse_state, model.output(&forward_state, samples[last]));

    for interval_index in (0..intervals_seconds.len()).rev() {
        let normalized_duration = time_scale * intervals_seconds[interval_index];
        let replay_state = load_state(forward_states, interval_index + 1);
        let segment = InputSegment::new(
            samples[interval_index + 1],
            samples[interval_index],
            InputModel::Linear,
        );
        let initial = TrajectoryReplayState {
            replay: replay_state,
            reverse: reverse_state,
        };
        let outcome = integrate(
            &replay_model,
            &initial,
            segment,
            normalized_duration,
            reverse_max_step,
            reverse_integration,
        )
        .map_err(|source| PrototypeError::Integration {
            pass: PrototypePass::Reverse,
            interval: interval_index,
            source,
        })?;
        reverse_state = outcome.state.reverse;
        // One coupled adaptive step evaluates the primitive Butterworth RHS
        // once for trajectory replay and once for the reverse filter.
        diagnostics
            .reverse
            .add_with_derivative_scale(&outcome.diagnostics, 2)?;
        let stored_forward = load_state(forward_states, interval_index);
        let forward_output = model.output(&stored_forward, samples[interval_index]);
        staged[interval_index] = model.output(&reverse_state, forward_output);
    }

    output.copy_from_slice(staged);
    Ok(diagnostics)
}

fn store_state<T: Scalar, const N: usize>(storage: &mut [T], knot: usize, state: &[T; N]) {
    let start = knot * N;
    storage[start..start + N].copy_from_slice(state);
}

fn load_state<T: Scalar, const N: usize>(storage: &[T], knot: usize) -> [T; N] {
    let start = knot * N;
    let mut state = [T::zero(); N];
    state.copy_from_slice(&storage[start..start + N]);
    state
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation, clippy::float_cmp)]

    use super::*;
    use crate::Tolerances;

    fn interval_for_q(q: f64, cutoff_hz: f64) -> f64 {
        q / (core::f64::consts::TAU * cutoff_hz)
    }

    fn run_constant<const N: usize, T>(value: T, interval: T)
    where
        T: Scalar + PartialEq,
        IntegrationConfig<T>: Default,
    {
        let samples = [value; 5];
        let intervals = [interval; 4];
        let mut output = [T::zero(); 5];
        let mut workspace = vec![T::zero(); trajectory_workspace_len::<N>(5).unwrap()];
        let diagnostics =
            trajectory_replay::<N, T>(T::one(), &intervals, &samples, &mut output, &mut workspace)
                .unwrap();

        assert_eq!(output, samples);
        assert_eq!(
            diagnostics.forward.derivative_evaluations,
            7 * diagnostics.forward.attempted_steps().unwrap()
        );
        assert_eq!(
            diagnostics.reverse.derivative_evaluations,
            14 * diagnostics.reverse.attempted_steps().unwrap()
        );
    }

    #[test]
    fn constant_records_are_bit_exact_for_every_order_and_scalar() {
        let interval_f64 = interval_for_q(core::f64::consts::PI / 32.0, 1.0);
        let interval_f32 = interval_f64 as f32;
        for value in [0.0, 0.75, -1.25] {
            run_constant::<1, f64>(value, interval_f64);
            run_constant::<2, f64>(value, interval_f64);
            run_constant::<4, f64>(value, interval_f64);
            run_constant::<8, f64>(value, interval_f64);
            run_constant::<1, f32>(value as f32, interval_f32);
            run_constant::<2, f32>(value as f32, interval_f32);
            run_constant::<4, f32>(value as f32, interval_f32);
            run_constant::<8, f32>(value as f32, interval_f32);
        }
    }

    #[test]
    fn first_order_two_sample_ramp_matches_the_complete_closed_form() {
        let q = core::f64::consts::PI / 64.0;
        let interval = interval_for_q(q, 1.0);
        let samples = [-0.375, 0.875];
        let mut output = [0.0; 2];
        let mut workspace = [0.0; 4];
        trajectory_replay::<1, f64>(1.0, &[interval], &samples, &mut output, &mut workspace)
            .unwrap();

        let alpha = 1.553_773_974_030_037_4;
        let duration = alpha * q;
        let slope = (samples[1] - samples[0]) / duration;
        let tail = 0.5 * slope * (1.0 - (-duration).exp());
        let expected = [samples[0] + tail, samples[1] - tail];
        for (actual, expected) in output.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 2.0e-9,
                "actual={actual}, expected={expected}, error={}",
                (actual - expected).abs()
            );
        }
    }

    #[test]
    fn collinear_partition_preserves_original_endpoints() {
        let interval = interval_for_q(core::f64::consts::PI / 32.0, 1.0);
        let samples = [-0.375, 0.875];
        let mut base_output = [0.0; 2];
        let mut base_workspace = [0.0; 4];
        trajectory_replay::<1, f64>(
            1.0,
            &[interval],
            &samples,
            &mut base_output,
            &mut base_workspace,
        )
        .unwrap();

        let refined_intervals = [interval / 3.0; 3];
        let refined_samples = [-0.375, 1.0 / 24.0, 11.0 / 24.0, 0.875];
        let mut refined_output = [0.0; 4];
        let mut refined_workspace = [0.0; 8];
        trajectory_replay::<1, f64>(
            1.0,
            &refined_intervals,
            &refined_samples,
            &mut refined_output,
            &mut refined_workspace,
        )
        .unwrap();

        assert!((base_output[0] - refined_output[0]).abs() <= 2.0e-9);
        assert!((base_output[1] - refined_output[3]).abs() <= 2.0e-9);
    }

    #[test]
    fn physical_cutoff_scaling_preserves_the_normalized_result() {
        let q = core::f64::consts::PI / 32.0;
        let samples = [-0.5, 0.25, 0.875];
        let run = |cutoff_hz: f64| {
            let intervals = [interval_for_q(q, cutoff_hz); 2];
            let mut output = [0.0; 3];
            let mut workspace = [0.0; 15];
            trajectory_replay::<4, f64>(
                cutoff_hz,
                &intervals,
                &samples,
                &mut output,
                &mut workspace,
            )
            .unwrap();
            output
        };

        let one = run(1.0);
        let thirty_seven = run(37.0);
        for (one, thirty_seven) in one.into_iter().zip(thirty_seven) {
            assert!((one - thirty_seven).abs() <= 2.0e-9);
        }
    }

    #[test]
    fn singleton_behavior_is_exact_and_does_no_integration_work() {
        let samples = [0.375];
        let mut output = [9.0];
        let mut workspace = [0.0; 9];
        let diagnostics =
            trajectory_replay::<8, f64>(1.0, &[], &samples, &mut output, &mut workspace).unwrap();

        assert_eq!(output, samples);
        assert_eq!(diagnostics, PrototypeDiagnostics::default());
    }

    #[test]
    fn workspace_failure_is_transactional_and_reports_the_exact_requirement() {
        let interval = interval_for_q(core::f64::consts::PI / 32.0, 1.0);
        let samples = [-0.375, 0.875];
        let mut output = [3.0, 4.0];
        let before = output;
        let mut workspace = [0.0; 9];

        assert_eq!(
            trajectory_replay::<4, f64>(1.0, &[interval], &samples, &mut output, &mut workspace,),
            Err(PrototypeError::WorkspaceTooSmall { required: 10 })
        );
        assert_eq!(output, before);
    }

    #[test]
    fn forward_failure_preserves_output() {
        let samples = [f64::MAX, -f64::MAX];
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 1.0)];
        let mut output = [3.0, 4.0];
        let before = output;
        let mut workspace = [0.0; 4];

        let error =
            trajectory_replay::<1, f64>(1.0, &intervals, &samples, &mut output, &mut workspace)
                .unwrap_err();
        assert!(matches!(
            error,
            PrototypeError::Integration {
                pass: PrototypePass::Forward,
                interval: 0,
                ..
            }
        ));
        assert_eq!(output, before);
    }

    #[test]
    fn reverse_failure_preserves_output() {
        let samples = [-1.0, 0.5, -0.25, 1.0, -0.75];
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 1.0); 4];
        let mut output = [3.0, 4.0, 5.0, 6.0, 7.0];
        let before = output;
        let mut workspace = [0.0; 45];
        let forward_integration = IntegrationConfig {
            max_step_attempts: MAX_INTERVAL_ATTEMPTS,
            ..IntegrationConfig::default()
        };
        let reverse_integration = IntegrationConfig {
            tolerances: Tolerances::new(1.0e-30, 1.0e-30),
            max_step_seconds: None,
            max_step_attempts: 1,
        };

        let error = trajectory_replay_with_configs::<8, f64>(
            1.0,
            &intervals,
            &samples,
            &mut output,
            &mut workspace,
            forward_integration,
            reverse_integration,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PrototypeError::Integration {
                pass: PrototypePass::Reverse,
                ..
            }
        ));
        assert_eq!(output, before);
    }

    #[test]
    fn workspace_length_arithmetic_is_checked() {
        assert_eq!(trajectory_workspace_len::<8>(5).unwrap(), 45);
        assert_eq!(
            trajectory_workspace_len::<8>(usize::MAX),
            Err(PrototypeError::WorkspaceSizeOverflow)
        );
    }
}
