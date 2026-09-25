use crate::model::HighPassModel;
use crate::streaming::StreamingCore;
use crate::{
    BuildError, InputModel, IntegrationConfig, IntegrationDiagnostics, ResetError, Response,
    Scalar, StreamingFilter, UpdateError,
};

/// Builder for a continuous-time high-pass filter.
#[derive(Clone, Copy, Debug)]
pub struct HighPassBuilder<const N: usize, T: Scalar = f64> {
    cutoff_hz: T,
    response: Response<T>,
    input_model: InputModel,
    initial_input: T,
    integration: IntegrationConfig<T>,
}

impl<const N: usize, T: Scalar> HighPassBuilder<N, T> {
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

    /// Selects the analog high-pass response family.
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
    /// Returns [`BuildError`] if the order is zero or any configuration value
    /// is invalid.
    pub fn build(self) -> Result<HighPass<N, T>, BuildError> {
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

        let model = HighPassModel::new(self.response)?;
        let runtime = StreamingCore::new(
            model,
            self.initial_input,
            angular_cutoff,
            self.input_model,
            self.integration,
        );
        Ok(HighPass {
            runtime,
            cutoff_hz: self.cutoff_hz,
            response: self.response,
        })
    }
}

/// An allocation-free, continuous-time high-pass filter.
///
/// `N` is the filter order. `cutoff_hz` denotes the −3 dB frequency of the
/// complete filter for every supported [`Response`]. High-pass responses have
/// direct feedthrough, so a newly supplied endpoint sample can affect the
/// returned output immediately.
#[derive(Clone, Copy, Debug)]
pub struct HighPass<const N: usize, T: Scalar = f64> {
    runtime: StreamingCore<T, HighPassModel<T, N>, N>,
    cutoff_hz: T,
    response: Response<T>,
}

impl<const N: usize, T: Scalar> HighPass<N, T> {
    /// Starts configuring a high-pass filter with total cutoff `cutoff_hz`.
    #[must_use]
    pub fn builder(cutoff_hz: T) -> HighPassBuilder<N, T>
    where
        IntegrationConfig<T>: Default,
    {
        HighPassBuilder::new(cutoff_hz)
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

    /// Advances the filter and returns its new endpoint output.
    ///
    /// Because a high-pass filter has direct feedthrough, the new `input`
    /// affects the returned endpoint even when [`InputModel::PreviousHold`]
    /// applied the preceding input throughout the elapsed interval.
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
    /// A high-pass filter's output is zero at every constant-input equilibrium.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError::NonFiniteInput`] if `input` is NaN or infinite.
    pub fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        <Self as StreamingFilter>::reset_to_steady(self, input)
    }
}

impl<const N: usize, T: Scalar> StreamingFilter for HighPass<N, T> {
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
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::float_cmp
    )]

    use approx::assert_relative_eq;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[test]
    fn constant_initial_input_starts_with_zero_output() {
        let mut filter = HighPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .initial_input(3.5)
            .build()
            .unwrap();
        assert_eq!(filter.output(), 0.0);
        assert_eq!(filter.update(3.5, 100.0).unwrap(), 0.0);
        assert!(filter.last_diagnostics().used_equilibrium_shortcut());
    }

    #[test]
    fn reset_to_steady_has_zero_output() {
        let mut filter = HighPass::<5>::builder(2.0)
            .response(Response::Bessel)
            .build()
            .unwrap();
        filter.update(1.0, 0.1).unwrap();
        filter.reset_to_steady(-4.0).unwrap();
        assert_eq!(filter.output(), 0.0);
        assert_eq!(filter.update(-4.0, 100.0).unwrap(), 0.0);
    }

    #[test]
    fn first_order_current_hold_step_matches_exact_decay() {
        let cutoff_hz = 3.0;
        let dt = 0.07;
        let mut filter = HighPass::<1>::builder(cutoff_hz)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let actual = filter.update(1.0, dt).unwrap();
        let expected = (-core::f64::consts::TAU * cutoff_hz * dt).exp();
        assert_relative_eq!(actual, expected, epsilon = 1.0e-8);
    }

    #[test]
    fn input_models_have_explicit_direct_feedthrough_semantics() {
        let dt = 0.1;
        let mut previous = HighPass::<1>::builder(1.0)
            .input_model(InputModel::PreviousHold)
            .build()
            .unwrap();
        let mut linear = HighPass::<1>::builder(1.0)
            .input_model(InputModel::Linear)
            .build()
            .unwrap();
        let mut current = HighPass::<1>::builder(1.0)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();

        let previous_output = previous.update(1.0, dt).unwrap();
        let linear_output = linear.update(1.0, dt).unwrap();
        let current_output = current.update(1.0, dt).unwrap();
        assert_eq!(previous_output, 1.0);
        assert!(previous_output > linear_output);
        assert!(linear_output > current_output);
        assert!(current_output > 0.0);
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
        let samples_per_period = 400;
        let periods = 60;
        let measured_periods = 5;
        let dt = 1.0 / (cutoff_hz * f64::from(samples_per_period));
        let mut filter = HighPass::<N>::builder(cutoff_hz)
            .response(response)
            .build()
            .unwrap();
        let mut in_phase = 0.0;
        let mut quadrature = 0.0;
        let first_measured = (periods - measured_periods) * samples_per_period;

        for index in 1..=periods * samples_per_period {
            let phase = core::f64::consts::TAU * f64::from(index) / f64::from(samples_per_period);
            let output = filter.update(phase.sin(), dt).unwrap();
            if index > first_measured {
                in_phase += output * phase.sin();
                quadrature += output * phase.cos();
            }
        }

        let measured_samples = f64::from(measured_periods * samples_per_period);
        let gain = 2.0 * in_phase.hypot(quadrature) / measured_samples;
        assert_relative_eq!(gain, 1.0 / 2.0_f64.sqrt(), epsilon = 8.0e-5);
    }

    #[test]
    fn f32_runtime_is_supported() {
        let mut filter = HighPass::<4, f32>::builder(10.0)
            .response(Response::Bessel)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        assert!(filter.update(1.0, 0.02).unwrap().is_finite());
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            max_shrink_iters: 4_096,
            ..ProptestConfig::default()
        })]

        #[test]
        fn physical_scaling_preserves_normalized_evolution(
            samples in prop::collection::vec(
                (-1_000.0_f64..1_000.0, 1.0e-6_f64..0.5),
                1..32,
            ),
            cutoff_hz in 0.01_f64..10_000.0,
            scale in 0.1_f64..10.0,
        ) {
            let mut original = HighPass::<5>::builder(cutoff_hz)
                .response(Response::Bessel)
                .build()
                .unwrap();
            let mut scaled = HighPass::<5>::builder(cutoff_hz * scale)
                .response(Response::Bessel)
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
    }
}
