use crate::Scalar;
use crate::model::ContinuousModel;
use crate::scalar::{from_f64, from_usize};

/// Normalized, unity-DC-gain Chebyshev Type I cascade.
///
/// Odd orders begin with one real pole. The remaining states are pairs
/// `[section_output, normalized_output_derivative]`. Every section has unity
/// DC gain, and the complete cascade is rescaled to −3 dB at normalized
/// frequency one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Chebyshev1<T, const N: usize> {
    real_rate: T,
    damping: [T; N],
    frequency_squared: [T; N],
    max_pole_magnitude: T,
}

impl<T: Scalar, const N: usize> Chebyshev1<T, N> {
    pub(crate) fn new(ripple_db: T) -> Option<Self> {
        let max_ripple_db = from_f64(3.010_299_956_639_812);
        if !ripple_db.is_finite() || ripple_db <= T::zero() || ripple_db >= max_ripple_db {
            return None;
        }

        let order = from_usize::<T>(N);
        let epsilon_squared = (T::LN_10() * ripple_db / from_f64(10.0)).exp_m1();
        let epsilon = epsilon_squared.sqrt();
        let mu = (T::one() / epsilon).asinh() / order;
        let sinh_mu = mu.sinh();
        let cosh_mu = mu.cosh();

        // The textbook prototype uses its ripple boundary as frequency one.
        // Find that prototype's frequency whose unity-DC-normalized gain is
        // −3 dB, then divide every pole by it.
        let chebyshev_at_cutoff = if N % 2 == 0 {
            (T::one() / epsilon_squared + from_f64(2.0)).sqrt()
        } else {
            T::one() / epsilon
        };
        let cutoff_scale = (chebyshev_at_cutoff.acosh() / order).cosh();

        if !epsilon.is_finite()
            || epsilon <= T::zero()
            || !cutoff_scale.is_finite()
            || cutoff_scale <= T::zero()
        {
            return None;
        }

        let mut damping = [T::zero(); N];
        let mut frequency_squared = [T::zero(); N];
        let section_count = N / 2;
        let denominator = from_f64::<T>(2.0) * order;
        let mut max_pole_magnitude = T::zero();

        for index in 0..section_count {
            let odd_number = from_usize::<T>(2 * index + 1);
            let angle = T::PI() * odd_number / denominator;
            let real_magnitude = sinh_mu * angle.sin() / cutoff_scale;
            let imaginary_magnitude = cosh_mu * angle.cos() / cutoff_scale;
            let pole_magnitude_squared =
                real_magnitude * real_magnitude + imaginary_magnitude * imaginary_magnitude;
            damping[index] = from_f64::<T>(2.0) * real_magnitude;
            frequency_squared[index] = pole_magnitude_squared;
            max_pole_magnitude = max_pole_magnitude.max(pole_magnitude_squared.sqrt());
        }

        // Put the lowest-Q sections first to reduce the signal reaching the
        // sections with the greatest internal resonance.
        for index in 1..section_count {
            let mut position = index;
            while position > 0
                && normalized_damping(damping[position - 1], frequency_squared[position - 1])
                    < normalized_damping(damping[position], frequency_squared[position])
            {
                damping.swap(position - 1, position);
                frequency_squared.swap(position - 1, position);
                position -= 1;
            }
        }

        let real_rate = if N % 2 == 1 {
            sinh_mu / cutoff_scale
        } else {
            T::zero()
        };
        max_pole_magnitude = max_pole_magnitude.max(real_rate);

        if !real_rate.is_finite()
            || damping[..section_count]
                .iter()
                .any(|value| !value.is_finite() || *value <= T::zero())
            || frequency_squared[..section_count]
                .iter()
                .any(|value| !value.is_finite() || *value <= T::zero())
            || !max_pole_magnitude.is_finite()
            || max_pole_magnitude <= T::zero()
        {
            return None;
        }

        Some(Self {
            real_rate,
            damping,
            frequency_squared,
            max_pole_magnitude,
        })
    }

    pub(crate) fn real_rate(&self) -> T {
        self.real_rate
    }

    pub(crate) fn section_coefficients(&self) -> (&[T], &[T]) {
        (&self.damping[..N / 2], &self.frequency_squared[..N / 2])
    }
}

fn normalized_damping<T: Scalar>(damping: T, frequency_squared: T) -> T {
    damping / frequency_squared.sqrt()
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for Chebyshev1<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        let mut state_index = 0;

        if N % 2 == 1 {
            derivative[0] = self.real_rate * (driving_signal - state[0]);
            driving_signal = state[0];
            state_index = 1;
        }

        let mut section_index = 0;
        while state_index + 1 < N {
            let output = state[state_index];
            let output_derivative = state[state_index + 1];
            derivative[state_index] = output_derivative;
            derivative[state_index + 1] = self.frequency_squared[section_index]
                * (driving_signal - output)
                - self.damping[section_index] * output_derivative;
            driving_signal = output;
            state_index += 2;
            section_index += 1;
        }
    }

    fn output(&self, state: &[T; N], _input: T) -> T {
        if N == 1 { state[0] } else { state[N - 2] }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        let mut state = [T::zero(); N];
        let mut state_index = 0;
        if N % 2 == 1 {
            state[0] = input;
            state_index = 1;
        }
        while state_index + 1 < N {
            state[state_index] = input;
            state_index += 2;
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

    type MagnitudeCheck = fn(f64, f64);

    #[rstest]
    #[case::order_1(check_magnitude::<1>)]
    #[case::order_2(check_magnitude::<2>)]
    #[case::order_3(check_magnitude::<3>)]
    #[case::order_4(check_magnitude::<4>)]
    #[case::order_8(check_magnitude::<8>)]
    #[case::order_15(check_magnitude::<15>)]
    fn cascade_matches_closed_form_magnitude(
        #[case] check: MagnitudeCheck,
        #[values(0.1, 0.5, 2.5)] ripple_db: f64,
        #[values(0.0, 0.1, 0.75, 1.0, 3.0)] frequency: f64,
    ) {
        check(ripple_db, frequency);
    }

    fn check_magnitude<const ORDER: usize>(ripple_db: f64, frequency: f64) {
        let model = Chebyshev1::<f64, ORDER>::new(ripple_db).unwrap();
        let actual = cascade_magnitude(&model, frequency);

        let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
        let dc_chebyshev_squared = if ORDER % 2 == 0 { 1.0 } else { 0.0 };
        let cutoff_scale = cutoff_scale::<ORDER>(epsilon_squared);
        let prototype_frequency = frequency * cutoff_scale;
        let chebyshev = if prototype_frequency <= 1.0 {
            (ORDER as f64 * prototype_frequency.acos()).cos()
        } else {
            (ORDER as f64 * prototype_frequency.acosh()).cosh()
        };
        let expected = ((1.0 + epsilon_squared * dc_chebyshev_squared)
            / (1.0 + epsilon_squared * chebyshev * chebyshev))
            .sqrt();

        assert_relative_eq!(actual, expected, epsilon = 3.0e-12, max_relative = 3.0e-12);
        if frequency == 1.0 {
            assert_relative_eq!(actual, 1.0 / 2.0_f64.sqrt(), epsilon = 3.0e-12);
        }
    }

    fn cascade_magnitude<const ORDER: usize>(
        model: &Chebyshev1<f64, ORDER>,
        frequency: f64,
    ) -> f64 {
        let mut actual = 1.0;
        if ORDER % 2 == 1 {
            actual *= model.real_rate / model.real_rate.hypot(frequency);
        }
        let (damping, frequency_squared) = model.section_coefficients();
        for (&section_damping, &section_frequency_squared) in damping.iter().zip(frequency_squared)
        {
            let real = section_frequency_squared - frequency * frequency;
            let imaginary = section_damping * frequency;
            actual *= section_frequency_squared / real.hypot(imaginary);
        }
        actual
    }

    fn cutoff_scale<const ORDER: usize>(epsilon_squared: f64) -> f64 {
        let epsilon = epsilon_squared.sqrt();
        let target = if ORDER % 2 == 0 {
            (1.0 / epsilon_squared + 2.0).sqrt()
        } else {
            1.0 / epsilon
        };
        (target.acosh() / ORDER as f64).cosh()
    }

    #[test]
    fn ripple_is_peak_to_peak_after_unity_dc_normalization() {
        let ripple_db = 0.5;
        let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();

        let odd = Chebyshev1::<f64, 3>::new(ripple_db).unwrap();
        let odd_edge = 1.0 / cutoff_scale::<3>(epsilon_squared);
        assert_relative_eq!(
            cascade_magnitude(&odd, odd_edge),
            10.0_f64.powf(-ripple_db / 20.0),
            epsilon = 2.0e-14
        );

        let even = Chebyshev1::<f64, 4>::new(ripple_db).unwrap();
        let even_peak = (core::f64::consts::PI / 8.0).cos() / cutoff_scale::<4>(epsilon_squared);
        assert_relative_eq!(
            cascade_magnitude(&even, even_peak),
            10.0_f64.powf(ripple_db / 20.0),
            epsilon = 2.0e-14
        );
    }

    #[rstest]
    #[case::odd_order(check_equilibrium::<3>)]
    #[case::even_order(check_equilibrium::<4>)]
    fn equilibrium_has_zero_derivative(#[case] check: fn()) {
        check();
    }

    fn check_equilibrium<const ORDER: usize>() {
        let model = Chebyshev1::<f64, ORDER>::new(0.5).unwrap();
        let state = model.equilibrium(2.5);
        let mut derivative = [1.0; ORDER];
        model.derivative(&state, 2.5, &mut derivative);
        assert!(derivative.iter().all(|value| *value == 0.0));
        assert_eq!(model.output(&state, 2.5), 2.5);
    }
}
