//! Private endpoint-replay negative control for forward-backward experiments.
//!
//! This deliberately stores only forward endpoint outputs, linearly joins
//! them for the reverse pass, and starts that reverse pass in equilibrium with
//! the final stored endpoint. It therefore does not model the continuing
//! forward-filter tail required by the physical batch contract. Nothing in
//! this module is public API or a candidate-selection claim.

use crate::model::{Butterworth, ContinuousModel};
use crate::scalar::{from_f64, from_usize};
use crate::solver::{InputSegment, SolverDiagnostics, integrate};
use crate::{InputModel, IntegrationConfig, Scalar, UpdateError};

const MAX_INTERVAL_ATTEMPTS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrototypePass {
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrototypeError {
    ZeroOrder,
    UnsupportedOrder,
    InvalidCutoff,
    InvalidIntegrationConfig,
    EmptyRecord,
    ShapeMismatch,
    OutputLength,
    WorkspaceSizeOverflow,
    WorkspaceTooSmall {
        required: usize,
    },
    NonFiniteSample {
        index: usize,
    },
    InvalidInterval {
        index: usize,
    },
    IntervalOutOfScope {
        index: usize,
    },
    Integration {
        pass: PrototypePass,
        interval: usize,
        source: UpdateError,
    },
    WorkCounterOverflow,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PrototypePassDiagnostics {
    pub(crate) accepted_steps: usize,
    pub(crate) rejected_steps: usize,
    pub(crate) derivative_evaluations: usize,
}

impl PrototypePassDiagnostics {
    fn add<T>(&mut self, diagnostics: &SolverDiagnostics<T>) -> Result<(), PrototypeError> {
        self.accepted_steps = self
            .accepted_steps
            .checked_add(diagnostics.accepted_steps)
            .ok_or(PrototypeError::WorkCounterOverflow)?;
        self.rejected_steps = self
            .rejected_steps
            .checked_add(diagnostics.rejected_steps)
            .ok_or(PrototypeError::WorkCounterOverflow)?;
        self.derivative_evaluations = self
            .derivative_evaluations
            .checked_add(diagnostics.derivative_evaluations)
            .ok_or(PrototypeError::WorkCounterOverflow)?;
        Ok(())
    }

    pub(crate) const fn attempted_steps(self) -> Option<usize> {
        self.accepted_steps.checked_add(self.rejected_steps)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PrototypeDiagnostics {
    pub(crate) forward: PrototypePassDiagnostics,
    pub(crate) reverse: PrototypePassDiagnostics,
}

impl PrototypeDiagnostics {
    pub(crate) fn attempted_steps(self) -> Option<usize> {
        self.forward
            .attempted_steps()?
            .checked_add(self.reverse.attempted_steps()?)
    }

    pub(crate) const fn derivative_evaluations(self) -> Option<usize> {
        self.forward
            .derivative_evaluations
            .checked_add(self.reverse.derivative_evaluations)
    }
}

pub(crate) fn endpoint_replay<const N: usize, T: Scalar>(
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
    endpoint_replay_with_config::<N, T>(
        cutoff_hz,
        intervals_seconds,
        samples,
        output,
        workspace,
        integration,
    )
}

fn endpoint_replay_with_config<const N: usize, T: Scalar>(
    cutoff_hz: T,
    intervals_seconds: &[T],
    samples: &[T],
    output: &mut [T],
    workspace: &mut [T],
    integration: IntegrationConfig<T>,
) -> Result<PrototypeDiagnostics, PrototypeError> {
    endpoint_replay_with_configs::<N, T>(
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
fn endpoint_replay_with_configs<const N: usize, T: Scalar>(
    cutoff_hz: T,
    intervals_seconds: &[T],
    samples: &[T],
    output: &mut [T],
    workspace: &mut [T],
    forward_integration: IntegrationConfig<T>,
    reverse_integration: IntegrationConfig<T>,
) -> Result<PrototypeDiagnostics, PrototypeError> {
    let (record_length, time_scale) = validate::<N, T>(
        cutoff_hz,
        intervals_seconds,
        samples,
        output.len(),
        workspace.len(),
        forward_integration,
    )?;
    if !reverse_integration.is_valid() {
        return Err(PrototypeError::InvalidIntegrationConfig);
    }
    let model = Butterworth::<T, N>::new();
    let forward_max_step = maximum_normalized_step(&model, time_scale, forward_integration)?;
    let reverse_max_step = maximum_normalized_step(&model, time_scale, reverse_integration)?;
    let required_workspace = record_length
        .checked_mul(2)
        .ok_or(PrototypeError::WorkspaceSizeOverflow)?;
    let (forward, staged) = workspace[..required_workspace].split_at_mut(record_length);

    let mut diagnostics = PrototypeDiagnostics::default();

    let mut state = model.equilibrium(samples[0]);
    forward[0] = model.output(&state, samples[0]);
    for interval_index in 0..intervals_seconds.len() {
        let normalized_duration = time_scale * intervals_seconds[interval_index];
        let segment = InputSegment::new(
            samples[interval_index],
            samples[interval_index + 1],
            InputModel::Linear,
        );
        let outcome = integrate(
            &model,
            &state,
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
        state = outcome.state;
        diagnostics.forward.add(&outcome.diagnostics)?;
        forward[interval_index + 1] = model.output(&state, samples[interval_index + 1]);
    }

    let last = record_length - 1;
    state = model.equilibrium(forward[last]);
    staged[last] = model.output(&state, forward[last]);
    for interval_index in (0..intervals_seconds.len()).rev() {
        let normalized_duration = time_scale * intervals_seconds[interval_index];
        let segment = InputSegment::new(
            forward[interval_index + 1],
            forward[interval_index],
            InputModel::Linear,
        );
        let outcome = integrate(
            &model,
            &state,
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
        state = outcome.state;
        diagnostics.reverse.add(&outcome.diagnostics)?;
        staged[interval_index] = model.output(&state, forward[interval_index]);
    }

    output.copy_from_slice(staged);
    Ok(diagnostics)
}

fn maximum_normalized_step<const N: usize, T: Scalar>(
    model: &Butterworth<T, N>,
    time_scale: T,
    integration: IntegrationConfig<T>,
) -> Result<T, PrototypeError> {
    let mut maximum = model.max_normalized_step();
    if let Some(max_step_seconds) = integration.max_step_seconds {
        let configured = time_scale * max_step_seconds;
        if !configured.is_finite() || configured <= T::zero() {
            return Err(PrototypeError::InvalidIntegrationConfig);
        }
        maximum = maximum.min(configured);
    }
    Ok(maximum)
}

fn validate<const N: usize, T: Scalar>(
    cutoff_hz: T,
    intervals_seconds: &[T],
    samples: &[T],
    output_length: usize,
    workspace_length: usize,
    integration: IntegrationConfig<T>,
) -> Result<(usize, T), PrototypeError> {
    if N == 0 {
        return Err(PrototypeError::ZeroOrder);
    }
    if !matches!(N, 1 | 2 | 4 | 8) {
        return Err(PrototypeError::UnsupportedOrder);
    }
    if !cutoff_hz.is_finite() || cutoff_hz <= T::zero() {
        return Err(PrototypeError::InvalidCutoff);
    }
    if !integration.is_valid() {
        return Err(PrototypeError::InvalidIntegrationConfig);
    }
    if samples.is_empty() {
        return Err(PrototypeError::EmptyRecord);
    }
    if intervals_seconds.len().checked_add(1) != Some(samples.len()) {
        return Err(PrototypeError::ShapeMismatch);
    }
    if output_length != samples.len() {
        return Err(PrototypeError::OutputLength);
    }
    let required_workspace = samples
        .len()
        .checked_mul(2)
        .ok_or(PrototypeError::WorkspaceSizeOverflow)?;
    if workspace_length < required_workspace {
        return Err(PrototypeError::WorkspaceTooSmall {
            required: required_workspace,
        });
    }
    if let Some(index) = samples.iter().position(|sample| !sample.is_finite()) {
        return Err(PrototypeError::NonFiniteSample { index });
    }

    let angular_cutoff = T::TAU() * cutoff_hz;
    if !angular_cutoff.is_finite() || angular_cutoff <= T::zero() {
        return Err(PrototypeError::InvalidCutoff);
    }
    let alpha = (from_f64::<T>(2.0).sqrt() - T::one())
        .powf(-T::one() / (from_f64::<T>(2.0) * from_usize::<T>(N)));
    let time_scale = alpha * angular_cutoff;
    if !time_scale.is_finite() || time_scale <= T::zero() {
        return Err(PrototypeError::InvalidCutoff);
    }

    let maximum_public_duration = T::PI() / from_f64::<T>(32.0);
    for (index, interval) in intervals_seconds.iter().copied().enumerate() {
        if !interval.is_finite() || interval <= T::zero() {
            return Err(PrototypeError::InvalidInterval { index });
        }
        let public_duration = angular_cutoff * interval;
        let pass_duration = time_scale * interval;
        if !public_duration.is_finite()
            || public_duration <= T::zero()
            || !pass_duration.is_finite()
            || pass_duration <= T::zero()
        {
            return Err(PrototypeError::InvalidInterval { index });
        }
        if public_duration > maximum_public_duration {
            return Err(PrototypeError::IntervalOutOfScope { index });
        }
    }

    Ok((samples.len(), time_scale))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation, clippy::float_cmp)]

    use super::*;
    use crate::Tolerances;
    use rstest::rstest;

    fn interval_for_q(q: f64, cutoff_hz: f64) -> f64 {
        q / (core::f64::consts::TAU * cutoff_hz)
    }

    fn run_constant<const N: usize, T>(value: T, interval: T)
    where
        T: Scalar + core::fmt::Debug + PartialEq,
        IntegrationConfig<T>: Default,
    {
        let samples = [value; 5];
        let intervals = [interval; 4];
        let mut output = [T::zero(); 5];
        let mut workspace = [T::zero(); 10];
        let diagnostics =
            endpoint_replay::<N, T>(T::one(), &intervals, &samples, &mut output, &mut workspace)
                .unwrap();
        assert_eq!(output, samples);
        assert!(diagnostics.attempted_steps().unwrap() <= 2 * intervals.len());
        assert_eq!(
            diagnostics.derivative_evaluations().unwrap(),
            7 * diagnostics.attempted_steps().unwrap()
        );
    }

    #[test]
    fn constant_records_are_bit_exact_for_every_prototype_order_and_scalar() {
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

    fn assert_compensated_scale<const N: usize>(expected_alpha: f64) {
        let cutoff_hz = 37.0;
        let (_, time_scale) =
            validate::<N, f64>(cutoff_hz, &[], &[0.0], 1, 2, IntegrationConfig::default()).unwrap();
        let actual_alpha = time_scale / (core::f64::consts::TAU * cutoff_hz);
        assert!((actual_alpha - expected_alpha).abs() <= 2.0e-15);
    }

    #[test]
    fn compensated_time_scale_matches_the_frozen_oracle_values() {
        assert_compensated_scale::<1>(1.553_773_974_030_037_4);
        assert_compensated_scale::<2>(1.246_504_702_770_927);
        assert_compensated_scale::<4>(1.116_469_750_047_410_3);
        assert_compensated_scale::<8>(1.056_631_321_723_623);
    }

    #[test]
    fn public_normalized_interval_scope_has_an_explicit_boundary() {
        let exact = interval_for_q(core::f64::consts::PI / 32.0, 1.0);
        validate::<1, f64>(
            1.0,
            &[exact],
            &[0.0, 1.0],
            2,
            4,
            IntegrationConfig::default(),
        )
        .unwrap();

        let above = f64::from_bits(exact.to_bits() + 1);
        assert_eq!(
            validate::<1, f64>(
                1.0,
                &[above],
                &[0.0, 1.0],
                2,
                4,
                IntegrationConfig::default(),
            ),
            Err(PrototypeError::IntervalOutOfScope { index: 0 })
        );
    }

    #[test]
    fn first_order_two_sample_result_matches_independent_closed_form() {
        let q = core::f64::consts::PI / 64.0;
        let interval = interval_for_q(q, 1.0);
        let samples = [-0.375, 0.875];
        let mut output = [0.0; 2];
        let mut workspace = [0.0; 4];
        endpoint_replay::<1, f64>(1.0, &[interval], &samples, &mut output, &mut workspace).unwrap();

        let alpha = 1.553_773_974_030_037_4;
        let duration = alpha * q;
        let response = 1.0 - (-duration).exp();
        let forward_slope = (samples[1] - samples[0]) / duration;
        let forward_endpoint = samples[1] - forward_slope * response;
        let reverse_slope = (samples[0] - forward_endpoint) / duration;
        let reverse_endpoint = samples[0] - reverse_slope * response;

        assert!(
            (output[0] - reverse_endpoint).abs() <= 2.0e-9,
            "reverse endpoint: actual={}, expected={}, error={}",
            output[0],
            reverse_endpoint,
            (output[0] - reverse_endpoint).abs()
        );
        assert!(
            (output[1] - forward_endpoint).abs() <= 2.0e-9,
            "forward endpoint: actual={}, expected={}, error={}",
            output[1],
            forward_endpoint,
            (output[1] - forward_endpoint).abs()
        );
    }

    #[test]
    fn singleton_behavior_is_exact_and_does_no_integration_work() {
        let samples = [0.375];
        let mut output = [9.0];
        let mut workspace = [0.0; 2];
        let diagnostics =
            endpoint_replay::<8, f64>(1.0, &[], &samples, &mut output, &mut workspace).unwrap();

        assert_eq!(output, samples);
        assert_eq!(diagnostics, PrototypeDiagnostics::default());
    }

    #[test]
    fn asymmetric_two_sample_probe_uses_caller_owned_storage() {
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 37.0)];
        let samples = [-0.375, 0.875];
        let mut output = [0.0; 2];
        let mut workspace = [0.0; 4];
        let diagnostics =
            endpoint_replay::<4, f64>(37.0, &intervals, &samples, &mut output, &mut workspace)
                .unwrap();

        assert!(output.iter().all(|value| value.is_finite()));
        assert!(diagnostics.forward.attempted_steps().unwrap() > 0);
        assert!(diagnostics.reverse.attempted_steps().unwrap() > 0);
    }

    #[rstest]
    #[case::zero_order(
        validate::<0, f64>(1.0, &[], &[0.0], 1, 2, IntegrationConfig::default()),
        PrototypeError::ZeroOrder
    )]
    #[case::unsupported_order(
        validate::<3, f64>(1.0, &[], &[0.0], 1, 2, IntegrationConfig::default()),
        PrototypeError::UnsupportedOrder
    )]
    #[case::invalid_cutoff(
        validate::<1, f64>(f64::NAN, &[], &[0.0], 1, 2, IntegrationConfig::default()),
        PrototypeError::InvalidCutoff
    )]
    #[case::empty_record(
        validate::<1, f64>(1.0, &[], &[], 0, 0, IntegrationConfig::default()),
        PrototypeError::EmptyRecord
    )]
    #[case::shape_mismatch(
        validate::<1, f64>(1.0, &[0.001], &[0.0], 1, 2, IntegrationConfig::default()),
        PrototypeError::ShapeMismatch
    )]
    #[case::output_length(
        validate::<1, f64>(1.0, &[], &[0.0], 0, 2, IntegrationConfig::default()),
        PrototypeError::OutputLength
    )]
    #[case::workspace_too_small(
        validate::<1, f64>(1.0, &[], &[0.0], 1, 1, IntegrationConfig::default()),
        PrototypeError::WorkspaceTooSmall { required: 2 }
    )]
    #[case::non_finite_sample(
        validate::<1, f64>(1.0, &[], &[f64::INFINITY], 1, 2, IntegrationConfig::default()),
        PrototypeError::NonFiniteSample { index: 0 }
    )]
    #[case::invalid_interval(
        validate::<1, f64>(1.0, &[0.0], &[0.0, 1.0], 2, 4, IntegrationConfig::default()),
        PrototypeError::InvalidInterval { index: 0 }
    )]
    #[case::out_of_scope_interval(
        validate::<1, f64>(1.0, &[1.0], &[0.0, 1.0], 2, 4, IntegrationConfig::default()),
        PrototypeError::IntervalOutOfScope { index: 0 }
    )]
    fn validation_is_explicit(
        #[case] actual: Result<(usize, f64), PrototypeError>,
        #[case] expected: PrototypeError,
    ) {
        assert_eq!(actual.unwrap_err(), expected);
    }

    #[test]
    fn invalid_configuration_is_rejected_before_work() {
        let integration = IntegrationConfig {
            tolerances: Tolerances::new(0.0, 1.0e-7),
            ..IntegrationConfig::default()
        };
        assert_eq!(
            validate::<1, f64>(1.0, &[], &[0.0], 1, 2, integration),
            Err(PrototypeError::InvalidIntegrationConfig)
        );
    }

    #[test]
    fn validation_failure_preserves_output() {
        let samples = [0.0, f64::NAN];
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 1.0)];
        let mut output = [3.0, 4.0];
        let before = output;
        let mut workspace = [0.0; 4];

        assert_eq!(
            endpoint_replay::<1, f64>(1.0, &intervals, &samples, &mut output, &mut workspace,),
            Err(PrototypeError::NonFiniteSample { index: 1 })
        );
        assert_eq!(output, before);
    }

    #[test]
    fn integration_failure_preserves_output() {
        let samples = [f64::MAX, -f64::MAX];
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 1.0)];
        let mut output = [3.0, 4.0];
        let before = output;
        let mut workspace = [0.0; 4];

        let error =
            endpoint_replay::<1, f64>(1.0, &intervals, &samples, &mut output, &mut workspace)
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
    fn reverse_integration_failure_preserves_output() {
        let samples = [-1.0, 0.5, -0.25, 1.0, -0.75];
        let intervals = [interval_for_q(core::f64::consts::PI / 32.0, 1.0); 4];
        let mut output = [3.0, 4.0, 5.0, 6.0, 7.0];
        let before = output;
        let mut workspace = [0.0; 10];
        let forward_integration = IntegrationConfig {
            max_step_attempts: MAX_INTERVAL_ATTEMPTS,
            ..IntegrationConfig::default()
        };
        let reverse_integration = IntegrationConfig {
            tolerances: Tolerances::new(1.0e-30, 1.0e-30),
            max_step_seconds: None,
            max_step_attempts: 1,
        };

        let error = endpoint_replay_with_configs::<8, f64>(
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
}
