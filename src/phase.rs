use core::fmt;
use core::ops::{Index, IndexMut};

use crate::model::{ContinuousModel, LowPassModel};
use crate::scalar::{from_f64, from_usize};
use crate::solver::{DifferentialModel, InputSegment, SolverState, integrate};
use crate::{
    InputModel, IntegrationDiagnostics, LowPass, ResetError, Response, Scalar, StreamingFilter,
    UpdateError,
};

/// Error returned while designing a streaming phase equalizer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PhaseEqualizationError {
    /// The band was outside the filter's passband or not finite and ordered.
    InvalidBand,
    /// The requested number of first-order sections was outside 1 through 4.
    InvalidSectionCount,
    /// First-order all-pass sections could not appreciably flatten this band.
    NoImprovement,
}

impl fmt::Display for PhaseEqualizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBand => "equalization band must lie within the low-pass passband",
            Self::InvalidSectionCount => "equalizer requires one through four sections",
            Self::NoImprovement => "first-order all-pass sections cannot flatten this band",
        })
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PhaseEqualizationError {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CoupledState<T, const N: usize, const M: usize> {
    base: [T; N],
    all_pass: [T; M],
}

impl<T, const N: usize, const M: usize> Index<usize> for CoupledState<T, N, M> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index < N {
            &self.base[index]
        } else {
            &self.all_pass[index - N]
        }
    }
}

impl<T, const N: usize, const M: usize> IndexMut<usize> for CoupledState<T, N, M> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index < N {
            &mut self.base[index]
        } else {
            &mut self.all_pass[index - N]
        }
    }
}

impl<T: Scalar, const N: usize, const M: usize> SolverState<T> for CoupledState<T, N, M> {
    const LEN: usize = N + M;

    fn zero() -> Self {
        Self {
            base: [T::zero(); N],
            all_pass: [T::zero(); M],
        }
    }
}

struct CoupledModel<'a, T, const N: usize, const M: usize> {
    base: &'a LowPassModel<T, N>,
    rates: &'a [T; M],
}

impl<T: Scalar, const N: usize, const M: usize> CoupledModel<'_, T, N, M> {
    fn output(&self, state: &CoupledState<T, N, M>, input: T) -> T {
        let mut driving = self.base.output(&state.base, input);
        for &section_state in &state.all_pass {
            driving = from_f64::<T>(2.0) * section_state - driving;
        }
        driving
    }

    fn max_normalized_step(&self) -> T {
        let fastest_all_pass = self.rates.iter().copied().fold(T::zero(), T::max);
        self.base
            .max_normalized_step()
            .min(T::one() / fastest_all_pass)
    }
}

impl<T: Scalar, const N: usize, const M: usize> DifferentialModel<T, CoupledState<T, N, M>>
    for CoupledModel<'_, T, N, M>
{
    fn derivative(
        &self,
        state: &CoupledState<T, N, M>,
        input: T,
        derivative: &mut CoupledState<T, N, M>,
    ) {
        ContinuousModel::derivative(self.base, &state.base, input, &mut derivative.base);
        let mut driving = self.base.output(&state.base, input);
        for index in 0..M {
            derivative.all_pass[index] = self.rates[index] * (driving - state.all_pass[index]);
            driving = from_f64::<T>(2.0) * state.all_pass[index] - driving;
        }
    }
}

/// A low-pass filter with an automatically designed delayed all-pass cascade.
///
/// Construct with [`LowPass::equalize_phase`]. The equalizer uses one through
/// four stable first-order all-pass sections. It reduces group-delay variation
/// over the selected band when construction succeeds, while adding latency.
/// The magnitude of the underlying continuous-time response is preserved.
/// Filter and equalizer states are integrated together at every RK stage,
/// including when samples have irregular spacing.
#[derive(Clone, Copy, Debug)]
pub struct PhaseEqualizedLowPass<const N: usize, const M: usize, T: Scalar = f64> {
    low_pass: LowPass<N, T>,
    all_pass_state: [T; M],
    rates: [T; M],
    output: T,
    at_equilibrium: bool,
    lower_hz: T,
    upper_hz: T,
}

impl<const N: usize, const M: usize, T: Scalar> PhaseEqualizedLowPass<N, M, T> {
    pub(crate) fn new(
        low_pass: LowPass<N, T>,
        lower_hz: T,
        upper_hz: T,
    ) -> Result<Self, PhaseEqualizationError> {
        if M == 0 || M > 4 {
            return Err(PhaseEqualizationError::InvalidSectionCount);
        }
        if !lower_hz.is_finite()
            || !upper_hz.is_finite()
            || lower_hz < T::zero()
            || upper_hz <= lower_hz
            || upper_hz > low_pass.cutoff_hz()
        {
            return Err(PhaseEqualizationError::InvalidBand);
        }

        let lower = lower_hz / low_pass.cutoff_hz();
        let upper = upper_hz / low_pass.cutoff_hz();
        let rates = design_rates(&low_pass.runtime.model, lower, upper)?;
        let initial_output = low_pass.output();
        Ok(Self {
            at_equilibrium: low_pass.runtime.at_equilibrium,
            low_pass,
            all_pass_state: [initial_output; M],
            rates,
            output: initial_output,
            lower_hz,
            upper_hz,
        })
    }

    /// Returns the lower frequency of the equalized band.
    #[must_use]
    pub const fn lower_hz(&self) -> T {
        self.lower_hz
    }

    /// Returns the upper frequency of the equalized band.
    #[must_use]
    pub const fn upper_hz(&self) -> T {
        self.upper_hz
    }

    /// Returns the original filter's −3 dB cutoff.
    #[must_use]
    pub const fn cutoff_hz(&self) -> T {
        self.low_pass.cutoff_hz()
    }

    /// Returns the original low-pass response family.
    #[must_use]
    pub const fn response(&self) -> Response<T> {
        self.low_pass.response()
    }

    /// Estimates the complete continuous-time group delay at a frequency.
    ///
    /// The frequency must lie in the designed band. The value includes the
    /// original filter's delay and the delay added by the equalizer.
    ///
    /// # Errors
    ///
    /// Returns [`PhaseEqualizationError::InvalidBand`] outside the band.
    pub fn group_delay_seconds(&self, frequency_hz: T) -> Result<T, PhaseEqualizationError> {
        if !frequency_hz.is_finite() || frequency_hz < self.lower_hz || frequency_hz > self.upper_hz
        {
            return Err(PhaseEqualizationError::InvalidBand);
        }
        let normalized_frequency = frequency_hz / self.low_pass.cutoff_hz();
        let mut normalized_delay = self
            .low_pass
            .runtime
            .model
            .group_delay(normalized_frequency);
        for &rate in &self.rates {
            normalized_delay = normalized_delay + all_pass_delay(rate, normalized_frequency);
        }
        Ok(normalized_delay / self.low_pass.runtime.time_scale)
    }

    /// Advances both the filter and equalizer through the complete interval.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] for invalid input or elapsed time, or if the
    /// coupled adaptive integration fails. Errors leave the object unchanged.
    pub fn update(&mut self, input: T, dt_seconds: T) -> Result<T, UpdateError> {
        <Self as StreamingFilter>::update(self, input, dt_seconds)
    }

    /// Returns the most recently accepted, phase-equalized output.
    #[must_use]
    pub const fn output(&self) -> T {
        self.output
    }

    /// Returns the most recent integration diagnostics.
    #[must_use]
    pub const fn last_diagnostics(&self) -> IntegrationDiagnostics<T> {
        self.low_pass.runtime.last_diagnostics
    }

    /// Returns both parts to zero-input equilibrium.
    pub fn reset(&mut self) {
        <Self as StreamingFilter>::reset(self);
    }

    /// Sets both parts to a constant-input equilibrium.
    ///
    /// # Errors
    ///
    /// Returns [`ResetError`] when the input is non-finite.
    pub fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        <Self as StreamingFilter>::reset_to_steady(self, input)
    }
}

impl<const N: usize, const M: usize, T: Scalar> StreamingFilter for PhaseEqualizedLowPass<N, M, T> {
    type Scalar = T;

    fn update(&mut self, input: T, dt_seconds: T) -> Result<T, UpdateError> {
        if !input.is_finite() {
            return Err(UpdateError::NonFiniteInput);
        }
        if !dt_seconds.is_finite() || dt_seconds <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }

        let normalized_duration = self.low_pass.runtime.time_scale * dt_seconds;
        if !normalized_duration.is_finite() || normalized_duration <= T::zero() {
            return Err(UpdateError::InvalidDeltaTime);
        }
        let old_input = self.low_pass.runtime.previous_input;
        let constant_at_equilibrium = self.at_equilibrium
            && match self.low_pass.runtime.input_model {
                InputModel::Linear | InputModel::CurrentHold => input == old_input,
                InputModel::PreviousHold => true,
            };
        if constant_at_equilibrium {
            let next_output = self.output;
            self.low_pass.runtime.previous_input = input;
            self.low_pass.runtime.last_diagnostics = IntegrationDiagnostics::equilibrium_shortcut();
            self.at_equilibrium = input == old_input;
            return Ok(next_output);
        }

        let model = CoupledModel {
            base: &self.low_pass.runtime.model,
            rates: &self.rates,
        };
        let mut max_step = model.max_normalized_step();
        if let Some(max_step_seconds) = self.low_pass.runtime.integration.max_step_seconds {
            let configured = self.low_pass.runtime.time_scale * max_step_seconds;
            if !configured.is_finite() {
                return Err(UpdateError::InvalidDeltaTime);
            }
            max_step = max_step.min(configured);
        }
        let segment = InputSegment::new(old_input, input, self.low_pass.runtime.input_model);
        let state = CoupledState {
            base: self.low_pass.runtime.state,
            all_pass: self.all_pass_state,
        };
        let outcome = integrate(
            &model,
            &state,
            segment,
            normalized_duration,
            max_step,
            self.low_pass.runtime.integration,
        )?;
        let next_output = model.output(&outcome.state, input);
        let base_output = model.base.output(&outcome.state.base, input);
        if !next_output.is_finite() || !base_output.is_finite() {
            return Err(UpdateError::NonFiniteState);
        }

        self.low_pass.runtime.state = outcome.state.base;
        self.all_pass_state = outcome.state.all_pass;
        self.low_pass.runtime.previous_input = input;
        self.low_pass.runtime.output = base_output;
        self.low_pass.runtime.last_diagnostics = IntegrationDiagnostics::from_solver(
            outcome.diagnostics,
            self.low_pass.runtime.time_scale,
        );
        self.low_pass.runtime.at_equilibrium = false;
        self.output = next_output;
        self.at_equilibrium = false;
        Ok(next_output)
    }

    fn output(&self) -> T {
        self.output
    }

    fn reset(&mut self) {
        self.low_pass.reset();
        self.all_pass_state = [self.low_pass.output(); M];
        self.output = self.low_pass.output();
        self.at_equilibrium = true;
    }

    fn reset_to_steady(&mut self, input: T) -> Result<(), ResetError> {
        self.low_pass.reset_to_steady(input)?;
        self.all_pass_state = [self.low_pass.output(); M];
        self.output = self.low_pass.output();
        self.at_equilibrium = true;
        Ok(())
    }
}

fn all_pass_delay<T: Scalar>(rate: T, frequency: T) -> T {
    from_f64::<T>(2.0) * rate / (rate * rate + frequency * frequency)
}

fn design_rates<T: Scalar, const N: usize, const M: usize>(
    model: &LowPassModel<T, N>,
    lower: T,
    upper: T,
) -> Result<[T; M], PhaseEqualizationError> {
    let minimum_rate = from_f64::<T>(0.05) * upper.max(from_f64(0.1));
    let maximum_rate = from_f64::<T>(20.0) * upper.max(from_f64(0.1));
    let multiplier = (maximum_rate / minimum_rate).powf(T::one() / from_usize(32));
    let mut rates = [maximum_rate; M];
    let baseline = delay_variance(model, lower, upper, &[]);
    if !baseline.is_finite() {
        return Err(PhaseEqualizationError::NoImprovement);
    }

    for _ in 0..3 {
        for section in 0..M {
            let mut best_rate = rates[section];
            let mut best_score = delay_variance(model, lower, upper, &rates);
            let mut candidate = minimum_rate;
            for _ in 0..=32 {
                rates[section] = candidate;
                let score = delay_variance(model, lower, upper, &rates);
                if score < best_score {
                    best_score = score;
                    best_rate = candidate;
                }
                candidate = candidate * multiplier;
            }
            rates[section] = best_rate;
        }
    }

    let achieved = delay_variance(model, lower, upper, &rates);
    if !achieved.is_finite() || achieved >= baseline * from_f64(0.95) {
        return Err(PhaseEqualizationError::NoImprovement);
    }
    Ok(rates)
}

fn delay_variance<T: Scalar, const N: usize>(
    model: &LowPassModel<T, N>,
    lower: T,
    upper: T,
    rates: &[T],
) -> T {
    let mut mean = T::zero();
    let mut sum_squared_deviations = T::zero();
    for index in 0..=32 {
        let frequency = lower + (upper - lower) * from_usize::<T>(index) / from_usize(32);
        let mut delay = model.group_delay(frequency);
        for &rate in rates {
            delay = delay + all_pass_delay(rate, frequency);
        }
        let old_difference = delay - mean;
        mean = mean + old_difference / from_usize::<T>(index + 1);
        sum_squared_deviations = old_difference.mul_add(delay - mean, sum_squared_deviations);
    }
    (sum_squared_deviations / from_usize(33)).max(T::zero())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use approx::assert_relative_eq;

    use super::*;
    use crate::{IntegrationConfig, Tolerances};

    #[test]
    fn butterworth_design_reduces_group_delay_spread_in_the_selected_band() {
        let base = LowPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let before = delay_variance(&base.runtime.model, 0.0, 1.0, &[]);
        let equalized = base.equalize_phase::<2>(0.0, 10.0).unwrap();
        let after = delay_variance(
            &equalized.low_pass.runtime.model,
            0.0,
            1.0,
            &equalized.rates,
        );
        assert!(after < before * 0.95, "before={before}, after={after}");
        assert!(equalized.group_delay_seconds(0.0).unwrap() > 0.0);
        assert!(equalized.group_delay_seconds(10.0).unwrap() > 0.0);
        assert_eq!(
            equalized.group_delay_seconds(11.0),
            Err(PhaseEqualizationError::InvalidBand)
        );
    }

    #[test]
    fn a_first_order_low_pass_cannot_be_flattened_by_this_all_pass_family() {
        let base = LowPass::<1>::builder(10.0).build().unwrap();
        assert!(matches!(
            base.equalize_phase::<1>(0.0, 10.0),
            Err(PhaseEqualizationError::NoImprovement)
        ));
    }

    #[test]
    fn invalid_bands_and_section_counts_fail_before_running() {
        let base = LowPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        assert!(matches!(
            base.equalize_phase::<0>(0.0, 10.0),
            Err(PhaseEqualizationError::InvalidSectionCount)
        ));
        assert!(matches!(
            base.equalize_phase::<5>(0.0, 10.0),
            Err(PhaseEqualizationError::InvalidSectionCount)
        ));
        assert!(matches!(
            base.equalize_phase::<2>(1.0, 11.0),
            Err(PhaseEqualizationError::InvalidBand)
        ));
        assert!(matches!(
            base.equalize_phase::<2>(f64::NAN, 10.0),
            Err(PhaseEqualizationError::InvalidBand)
        ));
    }

    #[test]
    fn steady_input_remains_exactly_steady_and_resets_clear_diagnostics() {
        let base = LowPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .initial_input(3.0)
            .build()
            .unwrap();
        let mut equalized = base.equalize_phase::<2>(0.0, 10.0).unwrap();
        assert_eq!(equalized.output(), 3.0);
        assert_eq!(equalized.update(3.0, 100.0).unwrap(), 3.0);
        assert!(equalized.last_diagnostics().used_equilibrium_shortcut());
        equalized.reset_to_steady(-2.0).unwrap();
        assert_eq!(equalized.output(), -2.0);
        assert_eq!(
            equalized.last_diagnostics(),
            IntegrationDiagnostics::default()
        );
    }

    #[test]
    fn integration_failure_is_transactional() {
        let config = IntegrationConfig {
            tolerances: Tolerances::default(),
            max_step_seconds: Some(0.001),
            max_step_attempts: 1,
        };
        let base = LowPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .integration(config)
            .build()
            .unwrap();
        let mut tested = base.equalize_phase::<2>(0.0, 10.0).unwrap();
        let untouched = tested;
        assert_eq!(
            tested.update(1.0, 0.1),
            Err(UpdateError::StepBudgetExceeded)
        );
        assert_eq!(tested.output(), untouched.output());
        assert_eq!(
            tested.low_pass.runtime.state,
            untouched.low_pass.runtime.state
        );
        assert_eq!(tested.all_pass_state, untouched.all_pass_state);
        assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
    }

    #[test]
    fn f32_design_and_runtime_are_supported() {
        let base = LowPass::<4, f32>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut equalized = base.equalize_phase::<1>(0.0, 10.0).unwrap();
        assert!(equalized.update(1.0, 0.005).unwrap().is_finite());
    }

    #[test]
    fn physical_frequency_scaling_preserves_coupled_evolution() {
        let base = LowPass::<4>::builder(10.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let scaled_base = LowPass::<4>::builder(100.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut original = base.equalize_phase::<2>(0.0, 10.0).unwrap();
        let mut scaled = scaled_base.equalize_phase::<2>(0.0, 100.0).unwrap();
        for &(input, dt) in &[(1.0, 0.005), (-0.5, 0.002), (0.3, 0.007)] {
            let left = original.update(input, dt).unwrap();
            let right = scaled.update(input, dt / 10.0).unwrap();
            assert_relative_eq!(left, right, epsilon = 1.0e-9);
        }
    }

    #[test]
    fn coupled_all_pass_preserves_streaming_sine_gain() {
        for frequency_hz in [2.5, 5.0, 10.0] {
            let mut base = LowPass::<4>::builder(10.0)
                .response(Response::Butterworth)
                .input_model(InputModel::Linear)
                .build()
                .unwrap();
            let mut equalized = base.equalize_phase::<2>(0.0, 10.0).unwrap();
            let samples_per_period = 200;
            let periods = 40;
            let measured_periods = 5;
            let dt = 1.0 / (frequency_hz * f64::from(samples_per_period));
            let first_measured = (periods - measured_periods) * samples_per_period;
            let mut base_in_phase = 0.0;
            let mut base_quadrature = 0.0;
            let mut equalized_in_phase = 0.0;
            let mut equalized_quadrature = 0.0;
            for index in 1..=periods * samples_per_period {
                let phase =
                    core::f64::consts::TAU * f64::from(index) / f64::from(samples_per_period);
                let input = phase.sin();
                let base_output = base.update(input, dt).unwrap();
                let equalized_output = equalized.update(input, dt).unwrap();
                if index > first_measured {
                    base_in_phase += base_output * phase.sin();
                    base_quadrature += base_output * phase.cos();
                    equalized_in_phase += equalized_output * phase.sin();
                    equalized_quadrature += equalized_output * phase.cos();
                }
            }
            let base_gain = base_in_phase.hypot(base_quadrature);
            let equalized_gain = equalized_in_phase.hypot(equalized_quadrature);
            assert_relative_eq!(equalized_gain, base_gain, max_relative = 1.0e-4);
        }
    }
}
