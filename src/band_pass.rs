use crate::model::BandPassModel;
use crate::streaming::StreamingCore;
use crate::{
    BuildError, InputModel, IntegrationConfig, IntegrationDiagnostics, ResetError, Response,
    Scalar, StreamingFilter, UpdateError,
};

/// Builder for a continuous-time band-pass filter.
#[derive(Clone, Copy, Debug)]
pub struct BandPassBuilder<const N: usize, T: Scalar = f64> {
    lower_cutoff_hz: T,
    upper_cutoff_hz: T,
    response: Response<T>,
    input_model: InputModel,
    initial_input: T,
    integration: IntegrationConfig<T>,
}

impl<const N: usize, T: Scalar> BandPassBuilder<N, T> {
    fn new(lower_cutoff_hz: T, upper_cutoff_hz: T) -> Self
    where
        IntegrationConfig<T>: Default,
    {
        Self {
            lower_cutoff_hz,
            upper_cutoff_hz,
            response: Response::default(),
            input_model: InputModel::default(),
            initial_input: T::zero(),
            integration: IntegrationConfig::default(),
        }
    }

    /// Selects the analog prototype response family.
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
    /// Returns [`BuildError`] if `N` is not positive and even, the lower and
    /// upper cutoff frequencies are invalid, or another configuration value is
    /// invalid.
    pub fn build(self) -> Result<BandPass<N, T>, BuildError> {
        if N == 0 {
            return Err(BuildError::ZeroOrder);
        }
        if N % 2 == 1 {
            return Err(BuildError::InvalidBandPassOrder);
        }
        if !self.lower_cutoff_hz.is_finite()
            || !self.upper_cutoff_hz.is_finite()
            || self.lower_cutoff_hz <= T::zero()
            || self.upper_cutoff_hz <= self.lower_cutoff_hz
        {
            return Err(BuildError::InvalidBandPassEdges);
        }
        if !self.initial_input.is_finite() {
            return Err(BuildError::InvalidInitialInput);
        }
        if !self.integration.is_valid() {
            return Err(BuildError::InvalidIntegrationConfig);
        }

        // sqrt(lower) * sqrt(upper) avoids overflowing the intermediate
        // product used by the geometric mean.
        let center_hz = self.lower_cutoff_hz.sqrt() * self.upper_cutoff_hz.sqrt();
        let bandwidth_hz = self.upper_cutoff_hz - self.lower_cutoff_hz;
        let fractional_bandwidth = bandwidth_hz / center_hz;
        let angular_center = T::TAU() * center_hz;
        if !center_hz.is_finite()
            || center_hz <= T::zero()
            || !fractional_bandwidth.is_finite()
            || fractional_bandwidth <= T::zero()
            || !angular_center.is_finite()
        {
            return Err(BuildError::InvalidBandPassEdges);
        }

        let model = BandPassModel::new(self.response, fractional_bandwidth)?;
        let runtime = StreamingCore::new(
            model,
            self.initial_input,
            angular_center,
            self.input_model,
            self.integration,
        );
        Ok(BandPass {
            runtime,
            lower_cutoff_hz: self.lower_cutoff_hz,
            upper_cutoff_hz: self.upper_cutoff_hz,
            center_hz,
            response: self.response,
        })
    }
}

/// An allocation-free, continuous-time band-pass filter.
///
/// `N` is the final filter order and must be positive and even. Analog
/// low-pass-to-band-pass transformation doubles the prototype order, so a
/// `BandPass::<4>` uses a second-order prototype and has fourth-order state.
/// Both cutoff arguments denote complete-filter −3 dB frequencies.
#[derive(Clone, Copy, Debug)]
pub struct BandPass<const N: usize, T: Scalar = f64> {
    runtime: StreamingCore<T, BandPassModel<T, N>, N>,
    lower_cutoff_hz: T,
    upper_cutoff_hz: T,
    center_hz: T,
    response: Response<T>,
}

impl<const N: usize, T: Scalar> BandPass<N, T> {
    /// Starts configuring a band-pass filter from its two −3 dB edges.
    #[must_use]
    pub fn builder(lower_cutoff_hz: T, upper_cutoff_hz: T) -> BandPassBuilder<N, T>
    where
        IntegrationConfig<T>: Default,
    {
        BandPassBuilder::new(lower_cutoff_hz, upper_cutoff_hz)
    }

    /// Returns the lower −3 dB cutoff frequency in hertz.
    #[must_use]
    pub const fn lower_cutoff_hz(&self) -> T {
        self.lower_cutoff_hz
    }

    /// Returns the upper −3 dB cutoff frequency in hertz.
    #[must_use]
    pub const fn upper_cutoff_hz(&self) -> T {
        self.upper_cutoff_hz
    }

    /// Returns the geometric center frequency in hertz.
    #[must_use]
    pub const fn center_hz(&self) -> T {
        self.center_hz
    }

    /// Returns the selected prototype response family.
    #[must_use]
    pub const fn response(&self) -> Response<T> {
        self.response
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
    /// A band-pass filter's output is zero at every constant-input equilibrium.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError::NonFiniteInput`] if `input` is NaN or infinite.
    pub fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        <Self as StreamingFilter>::reset_to_steady(self, input)
    }
}

impl<const N: usize, T: Scalar> StreamingFilter for BandPass<N, T> {
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
    #![allow(clippy::cast_precision_loss, clippy::float_cmp)]

    use approx::assert_relative_eq;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[test]
    fn builder_rejects_zero_and_odd_final_orders() {
        assert!(matches!(
            BandPass::<0>::builder(1.0, 2.0).build(),
            Err(BuildError::ZeroOrder)
        ));
        assert!(matches!(
            BandPass::<3>::builder(1.0, 2.0).build(),
            Err(BuildError::InvalidBandPassOrder)
        ));
    }

    #[rstest]
    #[case::zero_lower(0.0, 2.0)]
    #[case::negative_lower(-1.0, 2.0)]
    #[case::equal(2.0, 2.0)]
    #[case::reversed(3.0, 2.0)]
    #[case::nan_lower(f64::NAN, 2.0)]
    #[case::infinite_upper(1.0, f64::INFINITY)]
    fn builder_rejects_invalid_edges(#[case] lower: f64, #[case] upper: f64) {
        assert!(matches!(
            BandPass::<4>::builder(lower, upper).build(),
            Err(BuildError::InvalidBandPassEdges)
        ));
    }

    #[test]
    fn bessel_limit_applies_to_the_half_order_prototype() {
        assert!(
            BandPass::<50>::builder(1.0, 2.0)
                .response(Response::Bessel)
                .build()
                .is_ok()
        );
        assert!(matches!(
            BandPass::<52>::builder(1.0, 2.0)
                .response(Response::Bessel)
                .build(),
            Err(BuildError::UnsupportedBesselOrder)
        ));
    }

    #[test]
    fn constant_initial_input_starts_with_zero_output() {
        let mut filter = BandPass::<4>::builder(2.0, 8.0)
            .response(Response::Butterworth)
            .initial_input(3.5)
            .build()
            .unwrap();
        assert_eq!(filter.output(), 0.0);
        assert_eq!(filter.update(3.5, 100.0).unwrap(), 0.0);
        assert!(filter.last_diagnostics().used_equilibrium_shortcut());
    }

    type GainCheck = fn(Response, f64, f64);

    #[rstest]
    #[case::order_2(check_gain::<2>)]
    #[case::order_4(check_gain::<4>)]
    #[case::order_8(check_gain::<8>)]
    fn simulated_center_and_edge_gains(
        #[case] check: GainCheck,
        #[values(
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 }
        )]
        response: Response,
        #[values((2.0, 1.0 / 2.0_f64.sqrt()), (4.0, 1.0), (8.0, 1.0 / 2.0_f64.sqrt()))]
        frequency_and_gain: (f64, f64),
    ) {
        check(response, frequency_and_gain.0, frequency_and_gain.1);
    }

    fn check_gain<const N: usize>(response: Response, frequency_hz: f64, expected_gain: f64) {
        let samples_per_period = 400;
        let periods = 100;
        let measured_periods = 5;
        let dt = 1.0 / (frequency_hz * f64::from(samples_per_period));
        let mut filter = BandPass::<N>::builder(2.0, 8.0)
            .response(response)
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
        let gain = 2.0 * in_phase.hypot(quadrature) / measured_samples;
        assert_relative_eq!(gain, expected_gain, epsilon = 6.0e-5);
    }

    #[test]
    fn f32_runtime_is_supported() {
        let mut filter = BandPass::<8, f32>::builder(2.0, 8.0)
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
        fn physical_frequency_scaling_preserves_evolution(
            samples in prop::collection::vec(
                (-1_000.0_f64..1_000.0, 1.0e-6_f64..0.5),
                1..32,
            ),
            lower_hz in 0.01_f64..1_000.0,
            ratio in 1.01_f64..100.0,
            scale in 0.1_f64..10.0,
        ) {
            let upper_hz = lower_hz * ratio;
            let mut original = BandPass::<6>::builder(lower_hz, upper_hz)
                .response(Response::Bessel)
                .build()
                .unwrap();
            let mut scaled = BandPass::<6>::builder(lower_hz * scale, upper_hz * scale)
                .response(Response::Bessel)
                .build()
                .unwrap();
            let angular_center = core::f64::consts::TAU * (lower_hz * upper_hz).sqrt();

            for (input, normalized_dt) in samples {
                let dt = normalized_dt / angular_center;
                let original_output = original.update(input, dt).unwrap();
                let scaled_output = scaled.update(input, dt / scale).unwrap();
                // Local-error control scales with the internal state and input,
                // while band-pass output can be much smaller through
                // cancellation. Compare against the driving-signal scale.
                let scale = input
                    .abs()
                    .max(original_output.abs())
                    .max(scaled_output.abs())
                    .max(1.0);
                prop_assert!(
                    (original_output - scaled_output).abs() <= 5.0e-7 * scale,
                    "original={original_output}, scaled={scaled_output}, tolerance={}",
                    5.0e-7 * scale,
                );
            }
        }
    }
}
