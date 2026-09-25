use crate::model::LowPassModel;
use crate::streaming::StreamingCore;
use crate::{
    BuildError, InputModel, IntegrationConfig, IntegrationDiagnostics, ResetError, Response,
    Scalar, StreamingFilter, UpdateError,
};

/// Builder for a continuous-time low-pass filter.
#[derive(Clone, Copy, Debug)]
pub struct LowPassBuilder<const N: usize, T: Scalar = f64> {
    cutoff_hz: T,
    response: Response<T>,
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
            response: Response::default(),
            input_model: InputModel::default(),
            initial_input: T::zero(),
            integration: IntegrationConfig::default(),
        }
    }

    /// Selects the analog low-pass response family.
    #[must_use]
    pub const fn response(mut self, response: Response<T>) -> Self {
        self.response = response;
        self
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

        let model = LowPassModel::new(self.response)?;
        let runtime = StreamingCore::new(
            model,
            self.initial_input,
            angular_cutoff,
            self.input_model,
            self.integration,
        );
        Ok(LowPass {
            runtime,
            cutoff_hz: self.cutoff_hz,
            response: self.response,
        })
    }
}

/// An allocation-free, continuous-time low-pass filter.
///
/// `N` is the filter order. `cutoff_hz` denotes the −3 dB frequency of the
/// complete filter for every supported [`Response`].
#[derive(Clone, Copy, Debug)]
pub struct LowPass<const N: usize, T: Scalar = f64> {
    pub(crate) runtime: StreamingCore<T, LowPassModel<T, N>, N>,
    cutoff_hz: T,
    response: Response<T>,
}

impl<const N: usize, T: Scalar> LowPass<N, T> {
    /// Starts configuring a low-pass filter with total cutoff `cutoff_hz`.
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

    /// Returns the selected response family.
    #[must_use]
    pub const fn response(&self) -> Response<T> {
        self.response
    }

    /// Adds a delayed, continuous-time all-pass equalizer over a passband.
    ///
    /// `M` is the number of first-order all-pass sections (1 through 4).
    /// The design succeeds only when those sections measurably flatten the
    /// model's group delay in `lower_hz..=upper_hz`. The upper bound cannot
    /// exceed this low-pass filter's −3 dB cutoff. The returned filter is
    /// independent of `self`. If `self` has already processed samples, its
    /// current state is copied and the new equalizer starts in equilibrium
    /// with its current output.
    ///
    /// # Errors
    ///
    /// Returns [`crate::PhaseEqualizationError`] for an invalid band or
    /// section count, or when this section family cannot improve the delay.
    pub fn equalize_phase<const M: usize>(
        &self,
        lower_hz: T,
        upper_hz: T,
    ) -> Result<crate::PhaseEqualizedLowPass<N, M, T>, crate::PhaseEqualizationError> {
        crate::PhaseEqualizedLowPass::new(*self, lower_hz, upper_hz)
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
        self.runtime.output
    }

    /// Returns work performed by the most recent successful update.
    ///
    /// Construction and reset operations leave an empty snapshot. A failed
    /// update preserves the preceding successful snapshot.
    #[must_use]
    pub const fn last_diagnostics(&self) -> IntegrationDiagnostics<T> {
        self.runtime.last_diagnostics
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
        self.runtime.update(input, dt_seconds)
    }

    fn output(&self) -> T {
        self.runtime.output
    }

    fn reset(&mut self) {
        self.runtime.reset();
    }

    fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        self.runtime.reset_to_steady(input)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_lossless,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::float_cmp
    )]

    use approx::assert_relative_eq;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::Tolerances;
    use crate::model::RepeatedPole;

    #[test]
    fn builder_rejects_zero_order() {
        assert!(matches!(
            LowPass::<0>::builder(1.0).build(),
            Err(BuildError::ZeroOrder)
        ));
    }

    #[rstest]
    #[case::zero(0.0)]
    #[case::negative(-1.0)]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    fn builder_rejects_invalid_cutoff(#[case] cutoff: f64) {
        assert!(matches!(
            LowPass::<1>::builder(cutoff).build(),
            Err(BuildError::InvalidCutoff)
        ));
    }

    #[test]
    fn builder_rejects_invalid_initial_input() {
        assert!(matches!(
            LowPass::<1>::builder(1.0).initial_input(f64::NAN).build(),
            Err(BuildError::InvalidInitialInput)
        ));
    }

    #[test]
    fn builder_rejects_invalid_integration_config() {
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

    #[rstest]
    #[case::zero(0.0)]
    #[case::negative(-0.5)]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    #[case::three_db(3.010_299_956_639_812)]
    #[case::above_three_db(4.0)]
    fn builder_rejects_invalid_chebyshev_ripple(#[case] ripple_db: f64) {
        assert!(matches!(
            LowPass::<4>::builder(1.0)
                .response(Response::Chebyshev1 { ripple_db })
                .build(),
            Err(BuildError::InvalidPassbandRipple)
        ));
    }

    #[test]
    fn builder_rejects_bessel_order_above_the_validated_table() {
        assert!(matches!(
            LowPass::<26>::builder(1.0)
                .response(Response::Bessel)
                .build(),
            Err(BuildError::UnsupportedBesselOrder)
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
        let diagnostics = filter.last_diagnostics();
        assert!(diagnostics.used_equilibrium_shortcut());
        assert_eq!(diagnostics.accepted_steps(), 0);
        assert_eq!(diagnostics.rejected_steps(), 0);
        assert_eq!(diagnostics.derivative_evaluations(), 0);
        assert_eq!(diagnostics.smallest_accepted_step_seconds(), None);
        assert_eq!(diagnostics.largest_accepted_step_seconds(), None);

        filter.reset();
        assert_eq!(filter.output(), 0.0);
        assert_eq!(filter.runtime.state, [0.0; 4]);
        assert_eq!(filter.last_diagnostics(), IntegrationDiagnostics::default());

        filter.reset_to_steady(-2.0).unwrap();
        assert_eq!(filter.output(), -2.0);
        assert_eq!(filter.runtime.state, [-2.0; 4]);
        assert_eq!(filter.last_diagnostics(), IntegrationDiagnostics::default());
    }

    #[test]
    fn non_finite_input_is_transactional() {
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
        assert_eq!(filter.runtime.state, before.runtime.state);
        assert_eq!(filter.runtime.previous_input, before.runtime.previous_input);
        assert_eq!(filter.runtime.output, before.runtime.output);
        assert_eq!(
            filter.runtime.last_diagnostics,
            before.runtime.last_diagnostics
        );
        assert_eq!(filter.runtime.at_equilibrium, before.runtime.at_equilibrium);
    }

    #[rstest]
    #[case::zero(0.0)]
    #[case::negative(-1.0)]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    fn invalid_delta_time_is_transactional(#[case] invalid_dt: f64) {
        let mut filter = LowPass::<3>::builder(2.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        filter.update(1.0, 0.1).unwrap();
        let before = filter;

        assert_eq!(
            filter.update(2.0, invalid_dt),
            Err(UpdateError::InvalidDeltaTime)
        );
        assert_eq!(filter.runtime.state, before.runtime.state);
        assert_eq!(filter.runtime.previous_input, before.runtime.previous_input);
        assert_eq!(filter.runtime.output, before.runtime.output);
        assert_eq!(
            filter.runtime.last_diagnostics,
            before.runtime.last_diagnostics
        );
        assert_eq!(filter.runtime.at_equilibrium, before.runtime.at_equilibrium);
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
        assert_eq!(filter.runtime.state, before.runtime.state);
        assert_eq!(filter.runtime.previous_input, before.runtime.previous_input);
        assert_eq!(filter.runtime.output, before.runtime.output);
        assert_eq!(
            filter.runtime.last_diagnostics,
            before.runtime.last_diagnostics
        );
        assert_eq!(filter.runtime.at_equilibrium, before.runtime.at_equilibrium);
    }

    #[rstest]
    #[case::order_1(check_step_response::<1>)]
    #[case::order_2(check_step_response::<2>)]
    #[case::order_4(check_step_response::<4>)]
    #[case::order_8(check_step_response::<8>)]
    fn current_hold_matches_repeated_pole_step_response(#[case] check: fn()) {
        check();
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
        let scaled_time = RepeatedPole::<f64>::new(1).rate() * normalized_time;
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
    fn previous_hold_shortcuts_only_the_old_equilibrium_interval() {
        let mut filter = LowPass::<3>::builder(1.0)
            .input_model(InputModel::PreviousHold)
            .build()
            .unwrap();

        assert_eq!(filter.update(1.0, 100.0).unwrap(), 0.0);
        assert!(filter.last_diagnostics().used_equilibrium_shortcut());
        assert!(!filter.runtime.at_equilibrium);

        assert!(filter.update(1.0, 0.1).unwrap() > 0.0);
        assert!(!filter.last_diagnostics().used_equilibrium_shortcut());
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
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let output = filter.update(1.0, 0.02).unwrap();
        assert!(output.is_finite());
        assert!(output > 0.0 && output < 1.0);

        let mut chebyshev = LowPass::<4, f32>::builder(10.0)
            .response(Response::Chebyshev1 { ripple_db: 0.5 })
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        assert!(chebyshev.update(1.0, 0.02).unwrap().is_finite());

        let mut bessel = LowPass::<4, f32>::builder(10.0)
            .response(Response::Bessel)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        assert!(bessel.update(1.0, 0.02).unwrap().is_finite());
    }

    type CutoffCheck = fn(Response);

    #[rstest]
    #[case::order_1(check_cutoff_gain::<1>)]
    #[case::order_2(check_cutoff_gain::<2>)]
    #[case::order_4(check_cutoff_gain::<4>)]
    #[case::order_6(check_cutoff_gain::<6>)]
    fn simulated_cutoff_gain_matches_minus_three_db(
        #[case] check: CutoffCheck,
        #[values(
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 }
        )]
        response: Response,
    ) {
        check(response);
    }

    fn check_cutoff_gain<const N: usize>(response: Response) {
        let cutoff_hz = 5.0;
        let samples_per_period = 200;
        let periods = if matches!(response, Response::Chebyshev1 { .. }) {
            60
        } else {
            20
        };
        let dt = 1.0 / (cutoff_hz * samples_per_period as f64);
        let mut filter = LowPass::<N>::builder(cutoff_hz)
            .response(response)
            .build()
            .unwrap();
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

    type ButterworthGainCheck = fn(f64);

    #[rstest]
    #[case::order_2(check_butterworth_gain::<2>)]
    #[case::order_3(check_butterworth_gain::<3>)]
    #[case::order_4(check_butterworth_gain::<4>)]
    fn butterworth_streaming_response_matches_closed_form(
        #[case] check: ButterworthGainCheck,
        #[values(0.25, 1.0, 2.0)] frequency_ratio: f64,
    ) {
        check(frequency_ratio);
    }

    fn check_butterworth_gain<const N: usize>(frequency_ratio: f64) {
        let cutoff_hz = 2.0;
        let frequency_hz = cutoff_hz * frequency_ratio;
        let samples_per_period = 300;
        let periods = 40;
        let measured_periods = 8;
        let dt = 1.0 / (frequency_hz * f64::from(samples_per_period));
        let mut filter = LowPass::<N>::builder(cutoff_hz)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let first_measured = (periods - measured_periods) * samples_per_period;
        let mut in_phase = 0.0;
        let mut quadrature = 0.0;

        for index in 1..=periods * samples_per_period {
            let phase = core::f64::consts::TAU * f64::from(index) / f64::from(samples_per_period);
            let output = filter.update(phase.sin(), dt).unwrap();
            if index > first_measured {
                in_phase += output * phase.sin();
                quadrature += output * phase.cos();
            }
        }

        let measured_samples = f64::from(measured_periods * samples_per_period);
        let actual = 2.0 * in_phase.hypot(quadrature) / measured_samples;
        let expected = 1.0 / (1.0 + frequency_ratio.powi(2 * N as i32)).sqrt();
        assert_relative_eq!(actual, expected, epsilon = 1.5e-4, max_relative = 3.0e-4);
    }

    #[test]
    fn response_selection_is_preserved() {
        let default = LowPass::<2>::builder(10.0).build().unwrap();
        assert_eq!(default.response(), Response::RepeatedPole);

        let butterworth = LowPass::<2>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        assert_eq!(butterworth.response(), Response::Butterworth);

        let chebyshev = LowPass::<2>::builder(10.0)
            .response(Response::Chebyshev1 { ripple_db: 0.5 })
            .build()
            .unwrap();
        assert_eq!(
            chebyshev.response(),
            Response::Chebyshev1 { ripple_db: 0.5 }
        );

        let bessel = LowPass::<2>::builder(10.0)
            .response(Response::Bessel)
            .build()
            .unwrap();
        assert_eq!(bessel.response(), Response::Bessel);
    }

    #[test]
    fn butterworth_normalized_time_scaling_is_invariant() {
        let samples = [0.5, -0.25, 1.5, 1.0, -0.5];
        let dts = [0.01, 0.007, 0.013, 0.02, 0.005];
        let scale = 37.0;
        let mut slow = LowPass::<5>::builder(2.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut fast = LowPass::<5>::builder(2.0 * scale)
            .response(Response::Butterworth)
            .build()
            .unwrap();

        for (sample, dt) in samples.into_iter().zip(dts) {
            let slow_output = slow.update(sample, dt).unwrap();
            let fast_output = fast.update(sample, dt / scale).unwrap();
            assert_relative_eq!(slow_output, fast_output, epsilon = 2.0e-12);
        }
    }

    fn response_strategy() -> impl Strategy<Value = Response> {
        prop_oneof![
            Just(Response::RepeatedPole),
            Just(Response::Butterworth),
            Just(Response::Bessel),
            (0.05_f64..2.5).prop_map(|ripple_db| Response::Chebyshev1 { ripple_db }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            max_shrink_iters: 4_096,
            ..ProptestConfig::default()
        })]

        #[test]
        fn arbitrary_partitions_preserve_a_held_input(
            weights in prop::collection::vec(0.01_f64..1.0, 1..48),
            total_dt in 1.0e-4_f64..0.2,
            response in response_strategy(),
        ) {
            let weight_sum: f64 = weights.iter().sum();
            let mut single = LowPass::<5>::builder(3.0)
                .response(response)
                .input_model(InputModel::CurrentHold)
                .build()
                .unwrap();
            let expected = single.update(1.0, total_dt).unwrap();

            let mut partitioned = LowPass::<5>::builder(3.0)
                .response(response)
                .input_model(InputModel::CurrentHold)
                .build()
                .unwrap();
            let mut actual = 0.0;
            for weight in weights {
                actual = partitioned
                    .update(1.0, total_dt * weight / weight_sum)
                    .unwrap();
            }
            prop_assert!((actual - expected).abs() <= 5.0e-7);
        }

        #[test]
        fn arbitrary_partitions_preserve_a_linear_ramp(
            weights in prop::collection::vec(0.01_f64..1.0, 1..48),
            total_dt in 1.0e-4_f64..0.2,
            response in response_strategy(),
        ) {
            let weight_sum: f64 = weights.iter().sum();
            let mut single = LowPass::<4>::builder(2.0)
                .response(response)
                .build()
                .unwrap();
            let expected = single.update(1.0, total_dt).unwrap();

            let mut partitioned = LowPass::<4>::builder(2.0)
                .response(response)
                .build()
                .unwrap();
            let mut elapsed_fraction = 0.0;
            let mut actual = 0.0;
            for weight in weights {
                let fraction = weight / weight_sum;
                elapsed_fraction += fraction;
                actual = partitioned
                    .update(elapsed_fraction, total_dt * fraction)
                    .unwrap();
            }
            prop_assert!((actual - expected).abs() <= 5.0e-7);
        }

        #[test]
        fn arbitrary_irregular_stream_remains_finite(
            samples in prop::collection::vec(
                (-1_000.0_f64..1_000.0, 1.0e-6_f64..0.002),
                1..256,
            ),
            response in response_strategy(),
        ) {
            let mut filter = LowPass::<8>::builder(40.0)
                .response(response)
                .build()
                .unwrap();
            for (input, dt) in samples {
                let output = filter.update(input, dt);
                prop_assert!(output.is_ok());
                prop_assert!(output.unwrap().is_finite());
            }
        }

        #[test]
        fn arbitrary_physical_scaling_preserves_normalized_evolution(
            samples in prop::collection::vec(
                (-1_000.0_f64..1_000.0, 1.0e-6_f64..0.5),
                1..32,
            ),
            cutoff_hz in 0.01_f64..10_000.0,
            scale in 0.1_f64..10.0,
            response in response_strategy(),
        ) {
            let mut original = LowPass::<5>::builder(cutoff_hz)
                .response(response)
                .build()
                .unwrap();
            let mut scaled = LowPass::<5>::builder(cutoff_hz * scale)
                .response(response)
                .build()
                .unwrap();
            let angular_cutoff = core::f64::consts::TAU * cutoff_hz;

            for (input, normalized_dt) in samples {
                let dt = normalized_dt / angular_cutoff;
                let original_output = original.update(input, dt).unwrap();
                let scaled_output = scaled.update(input, dt / scale).unwrap();
                prop_assert!((original_output - scaled_output).abs() <= 2.0e-8);
            }
        }

        #[test]
        fn arbitrary_steady_reset_remains_steady(
            steady_input in -1.0e6_f64..1.0e6,
            cutoff_hz in 0.01_f64..10_000.0,
            dt in 1.0e-9_f64..1.0e6,
            response in response_strategy(),
        ) {
            let mut filter = LowPass::<7>::builder(cutoff_hz)
                .response(response)
                .build()
                .unwrap();
            filter.reset_to_steady(steady_input).unwrap();
            let output = filter.update(steady_input, dt).unwrap();
            prop_assert_eq!(output, steady_input);
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
        let diagnostics = filter.last_diagnostics();
        assert!(diagnostics.accepted_steps() >= 200);
        assert_eq!(
            diagnostics.attempted_steps(),
            diagnostics.accepted_steps() + diagnostics.rejected_steps()
        );
        assert!(diagnostics.derivative_evaluations() >= 7 * diagnostics.attempted_steps());
        assert!(diagnostics.smallest_accepted_step_seconds().unwrap() > 0.0);
        assert!(diagnostics.largest_accepted_step_seconds().unwrap() <= 0.000_25);
        assert!(!diagnostics.used_equilibrium_shortcut());

        let normalized_time = core::f64::consts::TAU * 8.0 * 0.05;
        let rate = RepeatedPole::<f64>::new(3).rate();
        let scaled_time = rate * normalized_time;
        let expected = 1.0 - (-scaled_time).exp() * (1.0 + scaled_time + scaled_time.powi(2) / 2.0);
        assert_relative_eq!(actual, expected, epsilon = 2.0e-10);
    }
}
