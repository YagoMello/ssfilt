//! Private streaming prototype for delayed Butterworth phase equalization.
//!
//! This module is test-only until the coupled realization passes the complete
//! waveform matrix. It deliberately contains no public coefficient table or
//! builder surface.

use crate::model::{Butterworth, ContinuousModel};
use crate::state::StateVector;
use crate::{Scalar, scalar::from_f64};

#[derive(Clone, Copy, Debug, PartialEq)]
struct CoupledState<T, const N: usize> {
    source: [T; N],
    all_pass: [T; 2],
}

impl<T: Scalar, const N: usize> StateVector<T> for CoupledState<T, N> {
    const LEN: usize = N + 2;

    fn zeros() -> Self {
        Self {
            source: [T::zero(); N],
            all_pass: [T::zero(); 2],
        }
    }

    fn component(&self, index: usize) -> T {
        if index < N {
            self.source[index]
        } else {
            self.all_pass[index - N]
        }
    }

    fn set_component(&mut self, index: usize, value: T) {
        if index < N {
            self.source[index] = value;
        } else {
            self.all_pass[index - N] = value;
        }
    }
}

/// Butterworth low-pass followed by one real second-order all-pass section.
///
/// `all_pass[0]` stores the position state scaled by the squared natural
/// frequency. This makes its constant-input equilibrium exactly equal to the
/// source output and avoids a division in equilibrium construction.
#[derive(Clone, Copy, Debug)]
struct EqualizedButterworth<T, const N: usize> {
    source: Butterworth<T, N>,
    sigma: T,
    natural_frequency_squared: T,
}

impl<T: Scalar, const N: usize> EqualizedButterworth<T, N> {
    fn new(sigma: T, eta: T) -> Self {
        debug_assert!(sigma.is_finite() && sigma > T::zero());
        debug_assert!(eta.is_finite() && eta >= T::zero());
        Self {
            source: Butterworth::new(),
            sigma,
            natural_frequency_squared: sigma.mul_add(sigma, eta * eta),
        }
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, CoupledState<T, N>>
    for EqualizedButterworth<T, N>
{
    fn derivative(
        &self,
        state: &CoupledState<T, N>,
        input: T,
        derivative: &mut CoupledState<T, N>,
    ) {
        self.source
            .derivative(&state.source, input, &mut derivative.source);
        let source_output = self.source.output(&state.source, input);
        let scaled_position = state.all_pass[0];
        let velocity = state.all_pass[1];
        derivative.all_pass[0] = self.natural_frequency_squared * velocity;
        derivative.all_pass[1] =
            source_output - from_f64::<T>(2.0) * self.sigma * velocity - scaled_position;
    }

    fn output(&self, state: &CoupledState<T, N>, input: T) -> T {
        let source_output = self.source.output(&state.source, input);
        source_output - from_f64::<T>(4.0) * self.sigma * state.all_pass[1]
    }

    fn equilibrium(&self, input: T) -> CoupledState<T, N> {
        CoupledState {
            source: self.source.equilibrium(input),
            all_pass: [input, T::zero()],
        }
    }

    fn max_normalized_step(&self) -> T {
        self.source
            .max_normalized_step()
            .min(T::one() / self.natural_frequency_squared.sqrt())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_precision_loss, clippy::float_cmp)]

    use approx::assert_relative_eq;
    use rstest::rstest;

    use super::*;
    use crate::solver::InputSegment;
    use crate::streaming::StreamingCore;
    use crate::{InputModel, IntegrationConfig, Tolerances, UpdateError};

    const ORDER_4_SIGMA: f64 = 0.774_282_178_267_629_5;
    const ORDER_4_ETA: f64 = 0.420_158_711_359_650_1;
    const ORDER_8_SIGMA: f64 = 0.631_643_162_371_801_7;
    const ORDER_8_ETA: f64 = 0.381_855_079_433_413_2;

    type Prototype<T, const N: usize> =
        StreamingCore<T, EqualizedButterworth<T, N>, CoupledState<T, N>>;

    fn prototype<T: Scalar, const N: usize>(
        cutoff_hz: T,
        sigma: f64,
        eta: f64,
        integration: IntegrationConfig<T>,
    ) -> Prototype<T, N> {
        StreamingCore::new(
            EqualizedButterworth::new(from_f64(sigma), from_f64(eta)),
            T::zero(),
            T::TAU() * cutoff_hz,
            InputModel::Linear,
            integration,
        )
        .unwrap()
    }

    #[rstest]
    #[case::order_4_f64(check_equilibrium::<f64, 4>, ORDER_4_SIGMA, ORDER_4_ETA)]
    #[case::order_8_f64(check_equilibrium::<f64, 8>, ORDER_8_SIGMA, ORDER_8_ETA)]
    #[case::order_4_f32(check_equilibrium::<f32, 4>, ORDER_4_SIGMA, ORDER_4_ETA)]
    #[case::order_8_f32(check_equilibrium::<f32, 8>, ORDER_8_SIGMA, ORDER_8_ETA)]
    fn constant_input_is_an_exact_equilibrium(
        #[case] check: fn(f64, f64),
        #[case] sigma: f64,
        #[case] eta: f64,
    ) {
        check(sigma, eta);
    }

    fn check_equilibrium<T: Scalar, const N: usize>(sigma: f64, eta: f64) {
        let model = EqualizedButterworth::<T, N>::new(from_f64(sigma), from_f64(eta));
        for input in [from_f64::<T>(-7.25), from_f64::<T>(3.5)] {
            let state = model.equilibrium(input);
            let mut derivative = CoupledState::zeros();
            model.derivative(&state, input, &mut derivative);
            assert!(derivative.source.iter().all(|value| *value == T::zero()));
            assert_eq!(derivative.all_pass, [T::zero(); 2]);
            assert_eq!(model.output(&state, input), input);
        }
    }

    #[test]
    fn all_pass_is_driven_by_the_source_stage_output() {
        let model = EqualizedButterworth::<f64, 4>::new(ORDER_4_SIGMA, ORDER_4_ETA);
        let state = CoupledState {
            source: [0.1, -0.2, 0.75, 0.3],
            all_pass: [0.0, 0.0],
        };
        let mut derivative = CoupledState::zeros();
        model.derivative(&state, 9.0, &mut derivative);

        assert_eq!(derivative.all_pass[1], 0.75);
        assert_ne!(derivative.all_pass[1], 9.0);
    }

    #[rstest]
    #[case::order_4(check_step_bound::<4>, ORDER_4_SIGMA, ORDER_4_ETA)]
    #[case::order_8(check_step_bound::<8>, ORDER_8_SIGMA, ORDER_8_ETA)]
    fn step_bound_includes_source_and_all_pass_poles(
        #[case] check: fn(f64, f64),
        #[case] sigma: f64,
        #[case] eta: f64,
    ) {
        check(sigma, eta);
    }

    fn check_step_bound<const N: usize>(sigma: f64, eta: f64) {
        let model = EqualizedButterworth::<f64, N>::new(sigma, eta);
        let all_pass_bound = 1.0 / sigma.hypot(eta);
        assert_eq!(model.max_normalized_step(), 1.0_f64.min(all_pass_bound));
    }

    #[test]
    fn faster_all_pass_reduces_the_source_step_bound() {
        let model = EqualizedButterworth::<f64, 4>::new(2.0, 0.0);
        assert_eq!(model.max_normalized_step(), 0.5);
    }

    #[test]
    fn physical_cutoff_scaling_changes_only_the_time_mapping() {
        let samples = [0.5, -0.25, 1.5, 1.0, -0.5];
        let normalized_dts = [0.07, 0.13, 0.09, 0.2, 0.05];
        let scale = 37.0;
        let mut slow = prototype::<f64, 8>(
            2.0,
            ORDER_8_SIGMA,
            ORDER_8_ETA,
            IntegrationConfig::default(),
        );
        let mut fast = prototype::<f64, 8>(
            2.0 * scale,
            ORDER_8_SIGMA,
            ORDER_8_ETA,
            IntegrationConfig::default(),
        );

        for (input, normalized_dt) in samples.into_iter().zip(normalized_dts) {
            let slow_dt = normalized_dt / (core::f64::consts::TAU * 2.0);
            let slow_output = slow.update(input, slow_dt).unwrap();
            let fast_output = fast.update(input, slow_dt / scale).unwrap();
            assert_relative_eq!(slow_output, fast_output, epsilon = 2.0e-8);
        }
    }

    #[test]
    fn linear_ramp_is_invariant_to_partitioning() {
        let integration = IntegrationConfig {
            tolerances: Tolerances::new(1.0e-12, 1.0e-10),
            ..IntegrationConfig::default()
        };
        let mut whole = prototype::<f64, 8>(1.0, ORDER_8_SIGMA, ORDER_8_ETA, integration);
        let mut split = whole;
        let total_time = 0.37;
        let final_input = 1.25;
        let whole_output = whole.update(final_input, total_time).unwrap();

        let partitions = [0.03, 0.04, 0.08, 0.02, 0.11, 0.09];
        let mut elapsed = 0.0;
        let mut split_output = 0.0;
        for dt in partitions {
            elapsed += dt;
            split_output = split
                .update(final_input * elapsed / total_time, dt)
                .unwrap();
        }

        assert_relative_eq!(split_output, whole_output, epsilon = 2.0e-10);
        for (split_state, whole_state) in split
            .state
            .source
            .iter()
            .chain(&split.state.all_pass)
            .zip(whole.state.source.iter().chain(&whole.state.all_pass))
        {
            assert_relative_eq!(split_state, whole_state, epsilon = 2.0e-10);
        }
    }

    #[test]
    fn failed_update_preserves_the_complete_coupled_snapshot() {
        let integration = IntegrationConfig {
            max_step_seconds: Some(1.0e-4),
            max_step_attempts: 1,
            ..IntegrationConfig::default()
        };
        let mut filter = prototype::<f64, 4>(10.0, ORDER_4_SIGMA, ORDER_4_ETA, integration);
        let before = filter;

        assert_eq!(
            filter.update(1.0, 0.1),
            Err(UpdateError::StepBudgetExceeded)
        );
        assert_eq!(filter.state, before.state);
        assert_eq!(filter.previous_input, before.previous_input);
        assert_eq!(filter.output, before.output);
        assert_eq!(filter.last_diagnostics, before.last_diagnostics);
        assert_eq!(filter.at_equilibrium, before.at_equilibrium);
    }

    #[rstest]
    #[case::order_4_f64(
        check_steady_tones::<f64, 4>,
        ORDER_4_SIGMA,
        ORDER_4_ETA,
        1.0e-7,
        2.0e-7
    )]
    #[case::order_8_f64(
        check_steady_tones::<f64, 8>,
        ORDER_8_SIGMA,
        ORDER_8_ETA,
        1.0e-7,
        2.0e-7
    )]
    #[case::order_4_f32(
        check_steady_tones::<f32, 4>,
        ORDER_4_SIGMA,
        ORDER_4_ETA,
        2.0e-5,
        3.0e-5
    )]
    #[case::order_8_f32(
        check_steady_tones::<f32, 8>,
        ORDER_8_SIGMA,
        ORDER_8_ETA,
        2.0e-5,
        3.0e-5
    )]
    fn streaming_all_pass_matches_analytic_steady_tones(
        #[case] check: fn(f64, f64, f64, f64),
        #[case] sigma: f64,
        #[case] eta: f64,
        #[case] gain_epsilon: f64,
        #[case] phase_epsilon: f64,
    ) {
        check(sigma, eta, gain_epsilon, phase_epsilon);
    }

    fn check_steady_tones<T: Scalar, const N: usize>(
        sigma: f64,
        eta: f64,
        gain_epsilon: f64,
        phase_epsilon: f64,
    ) where
        IntegrationConfig<T>: Default,
    {
        let rounded_sigma = from_f64::<T>(sigma).to_f64().unwrap();
        let rounded_eta = from_f64::<T>(eta).to_f64().unwrap();
        for frequency_ratio in [0.1, 0.4, 0.7] {
            let (source_gain, source_phase) = measure_source_tone::<T, N>(frequency_ratio);
            let (equalized_gain, equalized_phase) =
                measure_equalized_tone::<T, N>(frequency_ratio, sigma, eta);
            let natural_frequency_squared =
                rounded_sigma.mul_add(rounded_sigma, rounded_eta * rounded_eta);
            let expected_phase = -2.0
                * (2.0 * rounded_sigma * frequency_ratio)
                    .atan2(natural_frequency_squared - frequency_ratio * frequency_ratio);

            assert_relative_eq!(equalized_gain / source_gain, 1.0, epsilon = gain_epsilon);
            assert_relative_eq!(
                wrapped_phase(equalized_phase - source_phase - expected_phase),
                0.0,
                epsilon = phase_epsilon
            );
        }
    }

    fn measure_source_tone<T: Scalar, const N: usize>(frequency_ratio: f64) -> (f64, f64)
    where
        IntegrationConfig<T>: Default,
    {
        let model = Butterworth::<T, N>::new();
        let mut runtime = StreamingCore::new(
            model,
            T::zero(),
            T::TAU(),
            InputModel::Linear,
            IntegrationConfig::default(),
        )
        .unwrap();
        measure_tone(frequency_ratio, |input, dt| {
            runtime
                .update(from_f64(input), from_f64(dt))
                .unwrap()
                .to_f64()
                .unwrap()
        })
    }

    fn measure_equalized_tone<T: Scalar, const N: usize>(
        frequency_ratio: f64,
        sigma: f64,
        eta: f64,
    ) -> (f64, f64)
    where
        IntegrationConfig<T>: Default,
    {
        let mut runtime = prototype::<T, N>(T::one(), sigma, eta, IntegrationConfig::default());
        measure_tone(frequency_ratio, |input, dt| {
            runtime
                .update(from_f64(input), from_f64(dt))
                .unwrap()
                .to_f64()
                .unwrap()
        })
    }

    fn measure_tone(frequency_ratio: f64, mut update: impl FnMut(f64, f64) -> f64) -> (f64, f64) {
        let samples_per_period = 200;
        let periods = 24;
        let measured_periods = 6;
        let frequency_hz = frequency_ratio;
        let dt = 1.0 / (frequency_hz * f64::from(samples_per_period));
        let first_measured = (periods - measured_periods) * samples_per_period;
        let mut in_phase = 0.0;
        let mut quadrature = 0.0;

        for index in 1..=periods * samples_per_period {
            let phase = core::f64::consts::TAU * f64::from(index) / f64::from(samples_per_period);
            let output = update(phase.sin(), dt);
            if index > first_measured {
                in_phase += output * phase.sin();
                quadrature += output * phase.cos();
            }
        }

        let sample_count = f64::from(measured_periods * samples_per_period);
        let scale = 2.0 / sample_count;
        let in_phase = scale * in_phase;
        let quadrature = scale * quadrature;
        (in_phase.hypot(quadrature), quadrature.atan2(in_phase))
    }

    fn wrapped_phase(phase: f64) -> f64 {
        (phase + core::f64::consts::PI).rem_euclid(core::f64::consts::TAU) - core::f64::consts::PI
    }

    #[test]
    fn direct_solver_entry_works_with_the_coupled_state() {
        let model = EqualizedButterworth::<f64, 4>::new(ORDER_4_SIGMA, ORDER_4_ETA);
        let initial = model.equilibrium(0.0);
        let result = crate::solver::integrate(
            &model,
            &initial,
            InputSegment::new(0.0, 1.0, InputModel::Linear),
            0.2,
            model.max_normalized_step(),
            IntegrationConfig::default(),
        )
        .unwrap();
        assert!(result.state.all_finite());
        assert!(model.output(&result.state, 1.0).is_finite());
    }
}

#[cfg(test)]
mod waveform;
