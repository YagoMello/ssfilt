use crate::model::BandStopModel;
use crate::streaming::StreamingCore;
use crate::{
    BuildError, DormandPrince54, InputModel, IntegrationDiagnostics, Integrator, ResetError,
    Response, Scalar, StreamingFilter, UpdateError,
};

/// Builder for a continuous-time band-stop filter.
#[derive(Clone, Copy, Debug)]
pub struct BandStopBuilder<
    const N: usize,
    T: Scalar = f64,
    I: Integrator<T, N> = DormandPrince54<T>,
> {
    lower_cutoff_hz: T,
    upper_cutoff_hz: T,
    response: Response<T>,
    input_model: InputModel,
    initial_input: T,
    integrator: I,
}

impl<const N: usize, T: Scalar> BandStopBuilder<N, T, DormandPrince54<T>> {
    fn new(lower_cutoff_hz: T, upper_cutoff_hz: T) -> Self
    where
        DormandPrince54<T>: Default,
    {
        Self {
            lower_cutoff_hz,
            upper_cutoff_hz,
            response: Response::default(),
            input_model: InputModel::default(),
            initial_input: T::zero(),
            integrator: DormandPrince54::default(),
        }
    }
}

impl<const N: usize, T: Scalar, I: Integrator<T, N>> BandStopBuilder<N, T, I> {
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

    /// Replaces the complete integration policy.
    #[must_use]
    pub fn integrator<J: Integrator<T, N>>(self, integrator: J) -> BandStopBuilder<N, T, J> {
        BandStopBuilder {
            lower_cutoff_hz: self.lower_cutoff_hz,
            upper_cutoff_hz: self.upper_cutoff_hz,
            response: self.response,
            input_model: self.input_model,
            initial_input: self.initial_input,
            integrator,
        }
    }

    /// Validates the configuration and constructs the filter.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError`] if `N` is not positive and even, the lower and
    /// upper cutoff frequencies are invalid, or another configuration value is
    /// invalid.
    pub fn build(self) -> Result<BandStop<N, T, I>, BuildError> {
        if N == 0 {
            return Err(BuildError::ZeroOrder);
        }
        if !N.is_multiple_of(2) {
            return Err(BuildError::InvalidBandStopOrder);
        }
        if !self.lower_cutoff_hz.is_finite()
            || !self.upper_cutoff_hz.is_finite()
            || self.lower_cutoff_hz <= T::zero()
            || self.upper_cutoff_hz <= self.lower_cutoff_hz
        {
            return Err(BuildError::InvalidBandStopEdges);
        }
        if !self.initial_input.is_finite() {
            return Err(BuildError::InvalidInitialInput);
        }
        if !self.integrator.is_valid(crate::integrator::private::Token) {
            return Err(BuildError::InvalidIntegrationConfig);
        }

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
            return Err(BuildError::InvalidBandStopEdges);
        }

        let model = BandStopModel::new(self.response, fractional_bandwidth)?;
        let runtime = StreamingCore::new(
            model,
            self.initial_input,
            angular_center,
            self.input_model,
            self.integrator,
        )?;
        Ok(BandStop {
            runtime,
            lower_cutoff_hz: self.lower_cutoff_hz,
            upper_cutoff_hz: self.upper_cutoff_hz,
            center_hz,
            response: self.response,
        })
    }
}

/// An allocation-free, continuous-time band-stop filter.
///
/// `N` is the final filter order and must be positive and even. Analog
/// low-pass-to-band-stop transformation doubles the prototype order, so a
/// `BandStop::<4>` uses a second-order prototype and has fourth-order state.
/// Both cutoff arguments denote complete-filter −3 dB frequencies. The
/// geometric center is a transmission zero in the ideal continuous response.
#[derive(Clone, Copy, Debug)]
pub struct BandStop<const N: usize, T: Scalar = f64, I: Integrator<T, N> = DormandPrince54<T>> {
    runtime: StreamingCore<T, BandStopModel<T, N>, N, I>,
    lower_cutoff_hz: T,
    upper_cutoff_hz: T,
    center_hz: T,
    response: Response<T>,
}

impl<const N: usize, T: Scalar> BandStop<N, T, DormandPrince54<T>> {
    /// Starts configuring a band-stop filter from its two −3 dB edges.
    #[must_use]
    pub fn builder(lower_cutoff_hz: T, upper_cutoff_hz: T) -> BandStopBuilder<N, T>
    where
        DormandPrince54<T>: Default,
    {
        BandStopBuilder::new(lower_cutoff_hz, upper_cutoff_hz)
    }
}

impl<const N: usize, T: Scalar, I: Integrator<T, N>> BandStop<N, T, I> {
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
    /// The band-stop response has direct feedthrough, so the returned endpoint
    /// output uses the newly supplied input even with [`InputModel::PreviousHold`].
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] for invalid input or elapsed time, or if the
    /// integration cannot finish safely. The filter is unchanged on error.
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
    /// A band-stop filter passes every constant input unchanged at equilibrium.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError::NonFiniteInput`] if `input` is NaN or infinite,
    /// or [`ResetError::UnrepresentableSteadyState`] if its equilibrium cannot
    /// be represented.
    pub fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        <Self as StreamingFilter>::reset_to_steady(self, input)
    }
}

impl<const N: usize, T: Scalar, I: Integrator<T, N>> StreamingFilter for BandStop<N, T, I> {
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
    #![allow(clippy::float_cmp)]

    use approx::assert_relative_eq;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[test]
    fn builder_rejects_zero_and_odd_final_orders() {
        assert!(matches!(
            BandStop::<0>::builder(1.0, 2.0).build(),
            Err(BuildError::ZeroOrder)
        ));
        assert!(matches!(
            BandStop::<3>::builder(1.0, 2.0).build(),
            Err(BuildError::InvalidBandStopOrder)
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
            BandStop::<4>::builder(lower, upper).build(),
            Err(BuildError::InvalidBandStopEdges)
        ));
    }

    #[test]
    fn constant_initial_input_and_reset_are_exact() {
        let mut filter = BandStop::<8>::builder(2.0, 8.0)
            .response(Response::Chebyshev1 { ripple_db: 0.5 })
            .initial_input(3.25)
            .build()
            .unwrap();
        assert_eq!(filter.output(), 3.25);
        assert_eq!(filter.update(3.25, 1.0).unwrap(), 3.25);
        filter.reset_to_steady(-1.5).unwrap();
        assert_eq!(filter.output(), -1.5);
        assert_eq!(filter.update(-1.5, 1.0).unwrap(), -1.5);
    }

    #[test]
    fn previous_hold_uses_the_new_sample_for_endpoint_feedthrough() {
        let mut filter = BandStop::<2>::builder(10.0, 40.0)
            .response(Response::Butterworth)
            .input_model(InputModel::PreviousHold)
            .build()
            .unwrap();
        assert_eq!(filter.update(1.0, 0.01).unwrap(), 1.0);
    }

    #[test]
    fn second_order_current_hold_step_matches_the_exact_notch_response() {
        let lower_hz = 10.0;
        let upper_hz = 40.0;
        let center_hz = f64::sqrt(lower_hz * upper_hz);
        let damping = (upper_hz - lower_hz) / center_hz;
        let dt = 0.006;
        let normalized_time = core::f64::consts::TAU * center_hz * dt;
        let damped_frequency = (1.0 - damping * damping / 4.0).sqrt();
        let expected = 1.0
            - damping / damped_frequency
                * (-damping * normalized_time / 2.0).exp()
                * (damped_frequency * normalized_time).sin();
        let integrator = DormandPrince54 {
            absolute_tolerance: 1.0e-12,
            relative_tolerance: 1.0e-10,
            ..DormandPrince54::default()
        };
        let mut filter = BandStop::<2>::builder(lower_hz, upper_hz)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .integrator(integrator)
            .build()
            .unwrap();
        assert_relative_eq!(filter.update(1.0, dt).unwrap(), expected, epsilon = 2.0e-10);
    }

    #[test]
    fn bessel_limit_applies_to_the_half_order_prototype() {
        assert!(
            BandStop::<{ crate::MAX_BESSEL_BAND_STOP_ORDER }>::builder(1.0, 2.0)
                .response(Response::Bessel)
                .build()
                .is_ok()
        );
        assert!(matches!(
            BandStop::<52>::builder(1.0, 2.0)
                .response(Response::Bessel)
                .build(),
            Err(BuildError::UnsupportedBesselOrder)
        ));
    }

    #[test]
    fn f32_runtime_is_supported() {
        let mut filter = BandStop::<8, f32>::builder(2.0, 8.0)
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
            let integrator = DormandPrince54 {
                absolute_tolerance: 1.0e-12,
                relative_tolerance: 1.0e-9,
                ..DormandPrince54::default()
            };
            let mut original = BandStop::<6>::builder(lower_hz, upper_hz)
                .response(Response::Bessel)
                .integrator(integrator)
                .build()
                .unwrap();
            let mut scaled = BandStop::<6>::builder(lower_hz * scale, upper_hz * scale)
                .response(Response::Bessel)
                .integrator(integrator)
                .build()
                .unwrap();
            let angular_center = core::f64::consts::TAU * (lower_hz * upper_hz).sqrt();

            for (input, normalized_dt) in samples {
                let dt = normalized_dt / angular_center;
                let original_output = original.update(input, dt).unwrap();
                let scaled_output = scaled.update(input, dt / scale).unwrap();
                let output_scale = input
                    .abs()
                    .max(original_output.abs())
                    .max(scaled_output.abs())
                    .max(1.0);
                prop_assert!(
                    (original_output - scaled_output).abs() <= 5.0e-7 * output_scale,
                    "original={original_output}, scaled={scaled_output}, tolerance={}",
                    5.0e-7 * output_scale,
                );
            }
        }
    }
}
