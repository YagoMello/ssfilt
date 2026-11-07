use crate::scalar::from_f64;
use crate::{BuildError, Response, Scalar};

use super::ContinuousModel;
use super::band_transform::{PrototypeFactor, PrototypeFactors, factor_transformed_quadratic};

/// Cascade of DC-normalized band-stop sections in center-frequency time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandStopModel<T, const N: usize> {
    damping: [T; N],
    frequency_squared: [T; N],
    max_pole_magnitude: T,
}

impl<T: Scalar, const N: usize> BandStopModel<T, N> {
    pub(crate) fn new(response: Response<T>, fractional_bandwidth: T) -> Result<Self, BuildError> {
        let prototype_order = N / 2;
        let factors = PrototypeFactors::<T, N>::new(
            response,
            prototype_order,
            BuildError::InvalidBandStopEdges,
        )?;
        let mut model = Self {
            damping: [T::zero(); N],
            frequency_squared: [T::zero(); N],
            max_pole_magnitude: T::zero(),
        };
        let mut section_count = 0;

        for factor in factors.iter() {
            match factor {
                PrototypeFactor::Real { rate } => {
                    model.add_section(section_count, fractional_bandwidth / rate, T::one())?;
                    section_count += 1;
                }
                PrototypeFactor::Quadratic {
                    damping,
                    frequency_squared,
                } => {
                    // The band-stop denominator is the band-pass denominator
                    // of the reciprocal prototype: a -> a/b and b -> 1/b.
                    let sections = factor_transformed_quadratic(
                        damping / frequency_squared,
                        T::one() / frequency_squared,
                        fractional_bandwidth,
                    )
                    .ok_or(BuildError::InvalidBandStopEdges)?;
                    // The shared factorization returns the lower-frequency
                    // section first. Preserve each reciprocal pair so that it
                    // attenuates high-frequency internal signals before the
                    // second section restores unity total gain.
                    for section in sections {
                        model.add_section(
                            section_count,
                            section.damping,
                            section.frequency_squared,
                        )?;
                        section_count += 1;
                    }
                }
            }
        }

        if section_count != prototype_order
            || !model.max_pole_magnitude.is_finite()
            || model.max_pole_magnitude <= T::zero()
        {
            return Err(BuildError::InvalidBandStopEdges);
        }
        Ok(model)
    }

    fn add_section(
        &mut self,
        section: usize,
        damping: T,
        frequency_squared: T,
    ) -> Result<(), BuildError> {
        if section >= N / 2
            || !damping.is_finite()
            || damping <= T::zero()
            || !frequency_squared.is_finite()
            || frequency_squared <= T::zero()
        {
            return Err(BuildError::InvalidBandStopEdges);
        }

        self.damping[section] = damping;
        self.frequency_squared[section] = frequency_squared;
        let natural_frequency = frequency_squared.sqrt();
        let half_damping = damping / from_f64(2.0);
        let pole_magnitude = if half_damping > natural_frequency {
            let ratio = natural_frequency / half_damping;
            half_damping * (T::one() + (T::one() - ratio * ratio).max(T::zero()).sqrt())
        } else {
            natural_frequency
        };
        if !pole_magnitude.is_finite() || pole_magnitude <= T::zero() {
            return Err(BuildError::InvalidBandStopEdges);
        }
        self.max_pole_magnitude = self.max_pole_magnitude.max(pole_magnitude);
        Ok(())
    }

    fn section_output(&self, section: usize, input: T, position: T, velocity: T) -> T {
        input + (self.frequency_squared[section] - T::one()) * (input - position)
            - self.damping[section] * velocity
    }

    #[cfg(test)]
    fn section_coefficients(&self) -> (&[T], &[T]) {
        (&self.damping[..N / 2], &self.frequency_squared[..N / 2])
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for BandStopModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        for section in 0..N / 2 {
            let state_index = 2 * section;
            let position = state[state_index];
            let velocity = state[state_index + 1];
            derivative[state_index] = velocity;
            derivative[state_index + 1] = self.frequency_squared[section]
                * (driving_signal - position)
                - self.damping[section] * velocity;
            driving_signal = self.section_output(section, driving_signal, position, velocity);
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        let mut signal = input;
        for section in 0..N / 2 {
            let state_index = 2 * section;
            signal =
                self.section_output(section, signal, state[state_index], state[state_index + 1]);
        }
        signal
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        let mut state = [T::zero(); N];
        for section in 0..N / 2 {
            state[2 * section] = input;
        }
        state
    }

    fn max_normalized_step(&self) -> T {
        T::one() / self.max_pole_magnitude
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_precision_loss, clippy::float_cmp)]

    use approx::assert_relative_eq;
    use rstest::rstest;

    use super::*;

    const RESPONSES: [Response; 4] = [
        Response::RepeatedPole,
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ];

    #[test]
    fn constant_input_is_an_exact_unity_output_equilibrium() {
        for response in RESPONSES {
            let model = BandStopModel::<f64, 8>::new(response, 1.5).unwrap();
            let state = model.equilibrium(2.5);
            let mut derivative = [1.0; 8];
            model.derivative(&state, 2.5, &mut derivative);
            assert_eq!(derivative, [0.0; 8]);
            assert_eq!(model.output(&state, 2.5), 2.5);
        }
    }

    #[rstest]
    #[case::order_2(check_normalization::<2>)]
    #[case::order_4(check_normalization::<4>)]
    #[case::order_8(check_normalization::<8>)]
    fn center_and_edges_have_the_expected_gain(#[case] check: fn(Response, f64)) {
        for response in RESPONSES {
            check(response, 1.5);
        }
    }

    #[rstest]
    #[case::order_2(check_normalization::<2>)]
    #[case::order_8(check_normalization::<8>)]
    fn narrow_and_wide_bands_remain_normalized(
        #[case] check: fn(Response, f64),
        #[values(0.001, 0.1, 1.5, 10.0, 100.0)] fractional_bandwidth: f64,
        #[values(
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 }
        )]
        response: Response,
    ) {
        check(response, fractional_bandwidth);
    }

    fn check_normalization<const N: usize>(response: Response, fractional_bandwidth: f64) {
        let model = BandStopModel::<f64, N>::new(response, fractional_bandwidth).unwrap();
        let upper = f64::midpoint(fractional_bandwidth.hypot(2.0), fractional_bandwidth);
        let lower = 1.0 / upper;
        assert_relative_eq!(section_magnitude(&model, 0.0), 1.0, epsilon = 2.0e-13);
        assert_relative_eq!(
            section_magnitude(&model, lower),
            1.0 / 2.0_f64.sqrt(),
            epsilon = 5.0e-11,
            max_relative = 5.0e-11
        );
        assert_eq!(section_magnitude(&model, 1.0), 0.0);
        assert_relative_eq!(
            section_magnitude(&model, upper),
            1.0 / 2.0_f64.sqrt(),
            epsilon = 5.0e-11,
            max_relative = 5.0e-11
        );
        assert_relative_eq!(section_magnitude(&model, 1.0e9), 1.0, epsilon = 5.0e-11);
    }

    fn section_magnitude<const N: usize>(model: &BandStopModel<f64, N>, frequency: f64) -> f64 {
        let (damping, frequency_squared) = model.section_coefficients();
        damping.iter().zip(frequency_squared).fold(
            1.0,
            |magnitude, (&damping, &frequency_squared)| {
                let numerator = frequency_squared * (1.0 - frequency * frequency).abs();
                let denominator =
                    (frequency_squared - frequency * frequency).hypot(damping * frequency);
                magnitude * numerator / denominator
            },
        )
    }

    #[test]
    fn wide_band_step_cap_tracks_the_fastest_overdamped_pole() {
        let model = BandStopModel::<f64, 2>::new(Response::RepeatedPole, 100.0).unwrap();
        assert!(model.max_normalized_step() < 0.011);
    }
}
