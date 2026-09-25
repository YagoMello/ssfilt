use crate::model::{ContinuousModel, RepeatedPole};
use crate::solver::{InputSegment, integrate};
use crate::{
    BuildError, InputModel, IntegrationConfig, ResetError, Scalar, StreamingFilter, UpdateError,
};

/// Builder for a repeated-pole continuous-time low-pass filter.
#[derive(Clone, Copy, Debug)]
pub struct LowPassBuilder<const N: usize, T: Scalar = f64> {
    cutoff_hz: T,
    input_model: InputModel,
    initial_input: T,
    integration: IntegrationConfig<T>,
}

impl<const N: usize, T: Scalar> LowPassBuilder<N, T> {
    fn new(cutoff_hz: T) -> Self
    where
        IntegrationConfig<T>: Default,
    {
        Self {
            cutoff_hz,
            input_model: InputModel::default(),
            initial_input: T::zero(),
            integration: IntegrationConfig::default(),
        }
    }

    /// Selects how the input is reconstructed between samples.
    #[must_use]
    pub const fn input_model(mut self, input_model: InputModel) -> Self {
        self.input_model = input_model;
        self
    }

    /// Initializes the filter in equilibrium with a constant input.
    #[must_use]
    pub const fn initial_input(mut self, initial_input: T) -> Self {
        self.initial_input = initial_input;
        self
    }

    /// Replaces the adaptive integration configuration.
    #[must_use]
    pub const fn integration(mut self, integration: IntegrationConfig<T>) -> Self {
        self.integration = integration;
        self
    }

    /// Validates the configuration and constructs the filter.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError`] if the order is zero or any numeric
    /// configuration value is invalid.
    pub fn build(self) -> Result<LowPass<N, T>, BuildError> {
        if N == 0 {
            return Err(BuildError::ZeroOrder);
        }
        if !self.cutoff_hz.is_finite() || self.cutoff_hz <= T::zero() {
            return Err(BuildError::InvalidCutoff);
        }
        if !self.initial_input.is_finite() {
            return Err(BuildError::InvalidInitialInput);
        }
        if !self.integration.is_valid() {
            return Err(BuildError::InvalidIntegrationConfig);
        }

        let angular_cutoff = T::TAU() * self.cutoff_hz;
        if !angular_cutoff.is_finite() {
            return Err(BuildError::InvalidCutoff);
        }

        let model = RepeatedPole::new(N);
        let state =
            <RepeatedPole<T> as ContinuousModel<T, N>>::equilibrium(&model, self.initial_input);
        Ok(LowPass {
            model,
            state,
            previous_input: self.initial_input,
            output: self.initial_input,
            cutoff_hz: self.cutoff_hz,
            angular_cutoff,
            input_model: self.input_model,
            integration: self.integration,
        })
    }
}

/// An allocation-free, continuous-time repeated-pole low-pass filter.
///
/// `N` is the number of identical real poles. `cutoff_hz` denotes the −3 dB
/// frequency of the complete filter, not the location of each individual pole.
#[derive(Clone, Copy, Debug)]
pub struct LowPass<const N: usize, T: Scalar = f64> {
    model: RepeatedPole<T>,
    state: [T; N],
    previous_input: T,
    output: T,
    cutoff_hz: T,
    angular_cutoff: T,
    input_model: InputModel,
    integration: IntegrationConfig<T>,
}

impl<const N: usize, T: Scalar> LowPass<N, T> {
    /// Starts configuring a repeated-pole filter with total cutoff
    /// `cutoff_hz`.
    #[must_use]
    pub fn builder(cutoff_hz: T) -> LowPassBuilder<N, T>
    where
        IntegrationConfig<T>: Default,
    {
        LowPassBuilder::new(cutoff_hz)
    }

    /// Returns the total filter's −3 dB cutoff frequency in hertz.
    #[must_use]
    pub const fn cutoff_hz(&self) -> T {
        self.cutoff_hz
    }

    /// Advances the filter and returns its new output.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] for invalid input or elapsed time, or if the
    /// adaptive integration cannot finish safely. The filter is unchanged on
    /// error.
    pub fn update(&mut self, input: T, dt_seconds: T) -> Result<T, UpdateError> {
        <Self as StreamingFilter>::update(self, input, dt_seconds)
    }

    /// Returns the most recently accepted output.
    #[must_use]
    pub fn output(&self) -> T {
        self.output
    }

    /// Returns the filter to zero-input steady state.
    pub fn reset(&mut self) {
        <Self as StreamingFilter>::reset(self);
    }

    /// Sets the state to equilibrium with a constant input.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError::NonFiniteInput`] if `input` is NaN or infinite.
    pub fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        <Self as StreamingFilter>::reset_to_steady(self, input)
    }
}

impl<const N: usize, T: Scalar> StreamingFilter for LowPass<N, T> {
    type Scalar = T;

    fn update(&mut self, input: T, dt_seconds: T) -> Result<T, UpdateError> {
        if !input.is_finite() {
            return Err(UpdateError::NonFiniteInput);
        }
        if !dt_seconds.is_finite() || dt_seconds <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let normalized_duration = self.angular_cutoff * dt_seconds;
        if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let mut max_normalized_step =
            <RepeatedPole<T> as ContinuousModel<T, N>>::max_normalized_step(&self.model);
        if let Some(max_step_seconds) = self.integration.max_step_seconds {
            let configured = self.angular_cutoff * max_step_seconds;
            if !configured.is_finite() {
                return Err(UpdateError::InvalidDeltaTime);
            }
            max_normalized_step = max_normalized_step.min(configured);
        }

        let segment = InputSegment::new(self.previous_input, input, self.input_model);
        let next_state = integrate(
            &self.model,
            &self.state,
            segment,
            normalized_duration,
            max_normalized_step,
            self.integration,
        )?;
        let next_output = self.model.output(&next_state, input);
        if !next_output.is_finite() {
            return Err(UpdateError::NonFiniteState);
        }

        self.state = next_state;
        self.previous_input = input;
        self.output = next_output;
        Ok(next_output)
    }

    fn output(&self) -> T {
        self.output
    }

    fn reset(&mut self) {
        self.state = [T::zero(); N];
        self.previous_input = T::zero();
        self.output = T::zero();
    }

    fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        if !input.is_finite() {
            return Err(ResetError::NonFiniteInput);
        }
        self.state = self.model.equilibrium(input);
        self.previous_input = input;
        self.output = input;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_lossless, clippy::cast_precision_loss, clippy::float_cmp)]

    use approx::assert_relative_eq;

    use super::*;
    use crate::Tolerances;

    #[test]
    fn builder_rejects_invalid_values() {
        assert!(matches!(
            LowPass::<0>::builder(1.0).build(),
            Err(BuildError::ZeroOrder)
        ));

        for cutoff in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                LowPass::<1>::builder(cutoff).build(),
                Err(BuildError::InvalidCutoff)
            ));
        }

        assert!(matches!(
            LowPass::<1>::builder(1.0).initial_input(f64::NAN).build(),
            Err(BuildError::InvalidInitialInput)
        ));

        let invalid_integration = IntegrationConfig {
            tolerances: Tolerances::new(0.0, 1.0e-7),
            ..IntegrationConfig::default()
        };
        assert!(matches!(
            LowPass::<1>::builder(1.0)
                .integration(invalid_integration)
                .build(),
            Err(BuildError::InvalidIntegrationConfig)
        ));
    }

    #[test]
    fn initial_input_and_reset_to_steady_are_equilibria() {
        let mut filter = LowPass::<4>::builder(25.0)
            .initial_input(3.5)
            .build()
            .unwrap();
        assert_eq!(filter.output(), 3.5);
        assert_eq!(filter.update(3.5, 100.0).unwrap(), 3.5);

        filter.reset();
        assert_eq!(filter.output(), 0.0);
        assert_eq!(filter.state, [0.0; 4]);

        filter.reset_to_steady(-2.0).unwrap();
        assert_eq!(filter.output(), -2.0);
        assert_eq!(filter.state, [-2.0; 4]);
    }

    #[test]
    fn invalid_updates_are_transactional() {
        let mut filter = LowPass::<3>::builder(2.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        filter.update(1.0, 0.1).unwrap();
        let before = filter;

        assert_eq!(
            filter.update(f64::NAN, 0.1),
            Err(UpdateError::NonFiniteInput)
        );
        assert_eq!(filter.state, before.state);
        assert_eq!(filter.previous_input, before.previous_input);
        assert_eq!(filter.output, before.output);

        for invalid_dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                filter.update(2.0, invalid_dt),
                Err(UpdateError::InvalidDeltaTime)
            );
            assert_eq!(filter.state, before.state);
            assert_eq!(filter.previous_input, before.previous_input);
            assert_eq!(filter.output, before.output);
        }
    }

    #[test]
    fn step_budget_failure_is_transactional() {
        let integration = IntegrationConfig {
            max_step_seconds: Some(1.0e-4),
            max_step_attempts: 1,
            ..IntegrationConfig::default()
        };
        let mut filter = LowPass::<2>::builder(10.0)
            .input_model(InputModel::CurrentHold)
            .integration(integration)
            .build()
            .unwrap();
        let before = filter;

        assert_eq!(
            filter.update(1.0, 0.1),
            Err(UpdateError::StepBudgetExceeded)
        );
        assert_eq!(filter.state, before.state);
        assert_eq!(filter.previous_input, before.previous_input);
        assert_eq!(filter.output, before.output);
    }

    #[test]
    fn current_hold_matches_repeated_pole_step_response() {
        check_step_response::<1>();
        check_step_response::<2>();
        check_step_response::<4>();
        check_step_response::<8>();
    }

    fn check_step_response<const N: usize>() {
        let cutoff_hz = 3.0;
        let dt = 0.07;
        let mut filter = LowPass::<N>::builder(cutoff_hz)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let actual = filter.update(1.0, dt).unwrap();
        let normalized_time = core::f64::consts::TAU * cutoff_hz * dt;
        let rate = RepeatedPole::<f64>::new(N).rate();
        let scaled_time = rate * normalized_time;
        let mut term = 1.0;
        let mut sum = term;
        for index in 1..N {
            term *= scaled_time / index as f64;
            sum += term;
        }
        let expected = 1.0 - (-scaled_time).exp() * sum;
        assert_relative_eq!(actual, expected, epsilon = 2.0e-8, max_relative = 2.0e-7);
    }

    #[test]
    fn linear_input_matches_exact_first_order_ramp() {
        let cutoff_hz = 2.5;
        let dt = 0.03;
        let mut filter = LowPass::<1>::builder(cutoff_hz).build().unwrap();
        let actual = filter.update(1.0, dt).unwrap();
        let normalized_time = core::f64::consts::TAU * cutoff_hz * dt;
        let scaled_time = filter.model.rate() * normalized_time;
        let expected = 1.0 - -(-scaled_time).exp_m1() / scaled_time;
        assert_relative_eq!(actual, expected, epsilon = 1.0e-9);
    }

    #[test]
    fn hold_policies_have_distinct_timing_semantics() {
        let mut previous = LowPass::<1>::builder(1.0)
            .input_model(InputModel::PreviousHold)
            .build()
            .unwrap();
        assert_eq!(previous.update(1.0, 0.1).unwrap(), 0.0);
        assert!(previous.update(1.0, 0.1).unwrap() > 0.0);

        let mut current = LowPass::<1>::builder(1.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        assert!(current.update(1.0, 0.1).unwrap() > 0.0);
    }

    #[test]
    fn normalized_time_scaling_is_invariant() {
        let samples = [0.5, -0.25, 1.5, 1.0, -0.5];
        let dts = [0.01, 0.007, 0.013, 0.02, 0.005];
        let scale = 37.0;
        let mut slow = LowPass::<4>::builder(2.0).build().unwrap();
        let mut fast = LowPass::<4>::builder(2.0 * scale).build().unwrap();

        for (sample, dt) in samples.into_iter().zip(dts) {
            let slow_output = slow.update(sample, dt).unwrap();
            let fast_output = fast.update(sample, dt / scale).unwrap();
            assert_relative_eq!(slow_output, fast_output, epsilon = 2.0e-12);
        }
    }

    #[test]
    fn linear_ramp_is_invariant_to_partitioning() {
        let mut one_interval = LowPass::<3>::builder(4.0).build().unwrap();
        let whole = one_interval.update(1.0, 0.2).unwrap();

        let mut partitions = LowPass::<3>::builder(4.0).build().unwrap();
        let count = 20;
        let mut divided = 0.0;
        for index in 1..=count {
            divided = partitions
                .update(index as f64 / count as f64, 0.2 / count as f64)
                .unwrap();
        }
        assert_relative_eq!(whole, divided, epsilon = 2.0e-8);
    }

    #[test]
    fn f32_runtime_is_supported() {
        let mut filter = LowPass::<4, f32>::builder(10.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let output = filter.update(1.0, 0.02).unwrap();
        assert!(output.is_finite());
        assert!(output > 0.0 && output < 1.0);
    }

    #[test]
    fn simulated_cutoff_gain_matches_minus_three_db() {
        check_cutoff_gain::<1>();
        check_cutoff_gain::<2>();
        check_cutoff_gain::<4>();
        check_cutoff_gain::<6>();
    }

    fn check_cutoff_gain<const N: usize>() {
        let cutoff_hz = 5.0;
        let samples_per_period = 200;
        let periods = 20;
        let dt = 1.0 / (cutoff_hz * samples_per_period as f64);
        let mut filter = LowPass::<N>::builder(cutoff_hz).build().unwrap();
        let mut in_phase = 0.0;
        let mut quadrature = 0.0;
        let measured_periods = 5;
        let first_measured = (periods - measured_periods) * samples_per_period;

        for index in 1..=periods * samples_per_period {
            let phase = core::f64::consts::TAU * index as f64 / samples_per_period as f64;
            let output = filter.update(phase.sin(), dt).unwrap();
            if index > first_measured {
                in_phase += output * phase.sin();
                quadrature += output * phase.cos();
            }
        }

        let measured_samples = (measured_periods * samples_per_period) as f64;
        let gain = 2.0 * in_phase.hypot(quadrature) / measured_samples;
        assert_relative_eq!(gain, 1.0 / 2.0_f64.sqrt(), epsilon = 8.0e-5);
    }

    #[test]
    fn randomized_partitions_preserve_a_held_input() {
        for seed in 1..=12 {
            let weights = random_weights(seed);
            let weight_sum: f64 = weights.iter().sum();
            let total_dt = 0.15;

            let mut single = LowPass::<5>::builder(3.0)
                .input_model(InputModel::CurrentHold)
                .build()
                .unwrap();
            let expected = single.update(1.0, total_dt).unwrap();

            let mut partitioned = LowPass::<5>::builder(3.0)
                .input_model(InputModel::CurrentHold)
                .build()
                .unwrap();
            let mut actual = 0.0;
            for weight in weights {
                actual = partitioned
                    .update(1.0, total_dt * weight / weight_sum)
                    .unwrap();
            }
            assert_relative_eq!(actual, expected, epsilon = 2.0e-8);
        }
    }

    #[test]
    fn randomized_partitions_preserve_a_linear_ramp() {
        for seed in 20..=31 {
            let weights = random_weights(seed);
            let weight_sum: f64 = weights.iter().sum();
            let total_dt = 0.2;

            let mut single = LowPass::<4>::builder(2.0).build().unwrap();
            let expected = single.update(1.0, total_dt).unwrap();

            let mut partitioned = LowPass::<4>::builder(2.0).build().unwrap();
            let mut elapsed_fraction = 0.0;
            let mut actual = 0.0;
            for weight in weights {
                let fraction = weight / weight_sum;
                elapsed_fraction += fraction;
                actual = partitioned
                    .update(elapsed_fraction, total_dt * fraction)
                    .unwrap();
            }
            assert_relative_eq!(actual, expected, epsilon = 2.0e-8);
        }
    }

    #[test]
    fn long_irregular_stream_remains_finite() {
        let mut random = 0x5eed_1234_9876_abcd_u64;
        let mut filter = LowPass::<8>::builder(40.0).build().unwrap();

        for _ in 0..2_000 {
            let input = 2_000.0 * next_random(&mut random) - 1_000.0;
            let dt = 1.0e-6 + 0.002 * next_random(&mut random);
            assert!(filter.update(input, dt).unwrap().is_finite());
        }
    }

    #[test]
    fn configured_step_limit_subdivides_the_complete_interval() {
        let integration = IntegrationConfig {
            max_step_seconds: Some(0.000_25),
            ..IntegrationConfig::default()
        };
        let mut filter = LowPass::<3>::builder(8.0)
            .input_model(InputModel::CurrentHold)
            .integration(integration)
            .build()
            .unwrap();
        let actual = filter.update(1.0, 0.05).unwrap();

        let normalized_time = core::f64::consts::TAU * 8.0 * 0.05;
        let rate = filter.model.rate();
        let scaled_time = rate * normalized_time;
        let expected = 1.0 - (-scaled_time).exp() * (1.0 + scaled_time + scaled_time.powi(2) / 2.0);
        assert_relative_eq!(actual, expected, epsilon = 2.0e-10);
    }

    fn random_weights(seed: u64) -> [f64; 31] {
        let mut random = seed;
        let mut weights = [0.0; 31];
        for weight in &mut weights {
            *weight = 0.01 + next_random(&mut random);
        }
        weights
    }

    fn next_random(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*state >> 11) as f64) / ((1_u64 << 53) as f64)
    }
}
