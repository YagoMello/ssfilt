use crate::Scalar;
use crate::model::{ContinuousModel, bessel_table};
use crate::scalar::from_f64;

/// Magnitude-normalized Bessel cascade.
///
/// Odd orders begin with one real pole. The remaining states are pairs
/// `[section_output, normalized_output_derivative]`. Every section has unity
/// DC gain, and the complete response is −3 dB at normalized frequency one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bessel<T, const N: usize> {
    real_rate: T,
    damping: [T; N],
    frequency_squared: [T; N],
    max_pole_magnitude: T,
}

impl<T: Scalar, const N: usize> Bessel<T, N> {
    pub(crate) fn new() -> Option<Self> {
        let prototype = bessel_table::prototype(N)?;
        let mut damping = [T::zero(); N];
        let mut frequency_squared = [T::zero(); N];
        let mut max_pole_magnitude = T::zero();

        for (index, (&section_damping, &section_frequency_squared)) in prototype
            .damping
            .iter()
            .zip(prototype.frequency_squared)
            .enumerate()
        {
            damping[index] = from_f64(section_damping);
            frequency_squared[index] = from_f64(section_frequency_squared);
            max_pole_magnitude = max_pole_magnitude.max(frequency_squared[index].sqrt());
        }

        let real_rate = from_f64(prototype.real_rate);
        max_pole_magnitude = max_pole_magnitude.max(real_rate);

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

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for Bessel<T, N> {
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
    #![allow(clippy::float_cmp)]

    use approx::assert_relative_eq;
    use rstest::rstest;

    use super::*;

    type MagnitudeCheck = fn(f64);

    #[test]
    fn fourth_order_coefficients_match_reference_prototype() {
        let model = Bessel::<f64, 4>::new().unwrap();
        let (damping, frequency_squared) = model.section_coefficients();
        assert_relative_eq!(damping[0], 2.740_135_661_102_888, epsilon = 1.0e-15);
        assert_relative_eq!(damping[1], 1.990_417_528_700_546_3, epsilon = 1.0e-15);
        assert_relative_eq!(
            frequency_squared[0],
            2.045_390_691_015_643,
            epsilon = 1.0e-15
        );
        assert_relative_eq!(
            frequency_squared[1],
            2.570_755_324_809_459,
            epsilon = 1.0e-15
        );
    }

    #[rstest]
    #[case::order_1(check_magnitude::<1>)]
    #[case::order_2(check_magnitude::<2>)]
    #[case::order_3(check_magnitude::<3>)]
    #[case::order_4(check_magnitude::<4>)]
    #[case::order_8(check_magnitude::<8>)]
    #[case::order_16(check_magnitude::<16>)]
    #[case::order_25(check_magnitude::<25>)]
    fn cascade_has_unity_dc_and_minus_three_db_cutoff(#[case] check: MagnitudeCheck) {
        check(0.0);
        check(1.0);
    }

    fn check_magnitude<const ORDER: usize>(frequency: f64) {
        let model = Bessel::<f64, ORDER>::new().unwrap();
        let mut magnitude = 1.0;
        if ORDER % 2 == 1 {
            magnitude *= model.real_rate / model.real_rate.hypot(frequency);
        }
        let (damping, frequency_squared) = model.section_coefficients();
        for (&section_damping, &section_frequency_squared) in damping.iter().zip(frequency_squared)
        {
            let real = section_frequency_squared - frequency * frequency;
            let imaginary = section_damping * frequency;
            magnitude *= section_frequency_squared / real.hypot(imaginary);
        }
        let expected = if frequency == 0.0 {
            1.0
        } else {
            1.0 / 2.0_f64.sqrt()
        };
        assert_relative_eq!(
            magnitude,
            expected,
            epsilon = 1.0e-13,
            max_relative = 1.0e-13
        );
    }

    #[test]
    fn equilibrium_has_zero_derivative() {
        let model = Bessel::<f64, 7>::new().unwrap();
        let state = model.equilibrium(2.5);
        let mut derivative = [1.0; 7];
        model.derivative(&state, 2.5, &mut derivative);
        assert!(derivative.iter().all(|value| *value == 0.0));
        assert_eq!(model.output(&state, 2.5), 2.5);
    }

    #[test]
    fn unsupported_orders_do_not_construct() {
        assert!(Bessel::<f64, 0>::new().is_none());
        assert!(Bessel::<f64, 26>::new().is_none());
    }

    #[test]
    fn every_tabulated_prototype_is_stable_ordered_and_magnitude_normalized() {
        assert_eq!(bessel_table::MAX_ORDER, crate::MAX_BESSEL_ORDER);
        for order in 1..=bessel_table::MAX_ORDER {
            let prototype = bessel_table::prototype(order).unwrap();
            assert_eq!(prototype.damping.len(), order / 2);
            assert_eq!(prototype.frequency_squared.len(), order / 2);
            assert_eq!(prototype.real_rate > 0.0, order % 2 == 1);

            let mut magnitude = if order % 2 == 1 {
                prototype.real_rate / prototype.real_rate.hypot(1.0)
            } else {
                1.0
            };
            let mut preceding_normalized_damping = f64::INFINITY;
            for (&damping, &frequency_squared) in
                prototype.damping.iter().zip(prototype.frequency_squared)
            {
                assert!(damping.is_finite() && damping > 0.0);
                assert!(frequency_squared.is_finite() && frequency_squared > 0.0);
                let normalized_damping = damping / frequency_squared.sqrt();
                assert!(normalized_damping <= preceding_normalized_damping);
                preceding_normalized_damping = normalized_damping;
                magnitude *= frequency_squared / (frequency_squared - 1.0).hypot(damping);
            }
            assert_relative_eq!(
                magnitude,
                1.0 / 2.0_f64.sqrt(),
                epsilon = 1.0e-13,
                max_relative = 1.0e-13
            );
        }
    }

    #[test]
    fn eighth_order_group_delay_is_flat_near_dc() {
        let prototype = bessel_table::prototype(8).unwrap();
        let delay_at_dc = group_delay(&prototype, 0.0);
        let delay_at_half_cutoff = group_delay(&prototype, 0.5);
        assert_relative_eq!(delay_at_half_cutoff, delay_at_dc, max_relative = 4.0e-10);
    }

    fn group_delay(prototype: &bessel_table::Prototype, frequency: f64) -> f64 {
        let mut delay = if prototype.real_rate > 0.0 {
            prototype.real_rate
                / prototype
                    .real_rate
                    .mul_add(prototype.real_rate, frequency * frequency)
        } else {
            0.0
        };
        for (&damping, &frequency_squared) in
            prototype.damping.iter().zip(prototype.frequency_squared)
        {
            let denominator_real = frequency_squared - frequency * frequency;
            let denominator_imaginary = damping * frequency;
            delay += damping * (frequency_squared + frequency * frequency)
                / denominator_real.mul_add(
                    denominator_real,
                    denominator_imaginary * denominator_imaginary,
                );
        }
        delay
    }
}
