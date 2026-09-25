use crate::scalar::{from_f64, from_usize};
use crate::{BuildError, Response, Scalar};

use super::{ContinuousModel, bessel_table};

/// Cascade of real second-order band-pass sections in center-frequency time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandPassModel<T, const N: usize> {
    damping: [T; N],
    frequency_squared: [T; N],
    gain: [T; N],
    max_pole_magnitude: T,
}

impl<T: Scalar, const N: usize> BandPassModel<T, N> {
    pub(crate) fn new(response: Response<T>, fractional_bandwidth: T) -> Result<Self, BuildError> {
        let prototype_order = N / 2;
        let mut model = Self {
            damping: [T::zero(); N],
            frequency_squared: [T::zero(); N],
            gain: [T::zero(); N],
            max_pole_magnitude: T::zero(),
        };
        let result = match response {
            Response::RepeatedPole => {
                Ok(model.add_repeated_poles(prototype_order, fractional_bandwidth))
            }
            Response::Butterworth => model.add_butterworth(prototype_order, fractional_bandwidth),
            Response::Bessel => model.add_bessel(prototype_order, fractional_bandwidth),
            Response::Chebyshev1 { ripple_db } => {
                model.add_chebyshev1(prototype_order, fractional_bandwidth, ripple_db)
            }
        };
        let section_count = result?;
        if section_count != prototype_order || !model.is_valid(section_count) {
            return Err(BuildError::InvalidBandPassEdges);
        }
        model.sort_low_q_first(section_count);
        Ok(model)
    }

    fn add_repeated_poles(&mut self, order: usize, fractional_bandwidth: T) -> usize {
        let low_pass_rate = T::one() / (T::LN_2() / from_usize::<T>(order)).exp_m1().sqrt();
        let coefficient = fractional_bandwidth * low_pass_rate;
        for index in 0..order {
            self.damping[index] = coefficient;
            self.frequency_squared[index] = T::one();
            self.gain[index] = coefficient;
        }
        order
    }

    fn add_butterworth(
        &mut self,
        order: usize,
        fractional_bandwidth: T,
    ) -> Result<usize, BuildError> {
        let mut section = 0;
        if order % 2 == 1 {
            self.add_real_pole(section, T::one(), fractional_bandwidth);
            section += 1;
        }
        let denominator = from_f64::<T>(2.0) * from_usize::<T>(order);
        for index in 0..order / 2 {
            let angle = T::PI() * from_usize::<T>(2 * index + 1) / denominator;
            let damping = from_f64::<T>(2.0) * angle.sin();
            self.add_complex_pair(&mut section, damping, T::one(), fractional_bandwidth)?;
        }
        Ok(section)
    }

    fn add_bessel(&mut self, order: usize, fractional_bandwidth: T) -> Result<usize, BuildError> {
        let prototype = bessel_table::prototype(order).ok_or(BuildError::UnsupportedBesselOrder)?;
        let mut section = 0;
        if order % 2 == 1 {
            self.add_real_pole(section, from_f64(prototype.real_rate), fractional_bandwidth);
            section += 1;
        }
        for (&damping, &frequency_squared) in
            prototype.damping.iter().zip(prototype.frequency_squared)
        {
            self.add_complex_pair(
                &mut section,
                from_f64(damping),
                from_f64(frequency_squared),
                fractional_bandwidth,
            )?;
        }
        Ok(section)
    }

    fn add_chebyshev1(
        &mut self,
        order: usize,
        fractional_bandwidth: T,
        ripple_db: T,
    ) -> Result<usize, BuildError> {
        let max_ripple_db = from_f64(3.010_299_956_639_812);
        if !ripple_db.is_finite() || ripple_db <= T::zero() || ripple_db >= max_ripple_db {
            return Err(BuildError::InvalidPassbandRipple);
        }

        let order_scalar = from_usize::<T>(order);
        let epsilon_squared = (T::LN_10() * ripple_db / from_f64(10.0)).exp_m1();
        let epsilon = epsilon_squared.sqrt();
        let mu = (T::one() / epsilon).asinh() / order_scalar;
        let sinh_mu = mu.sinh();
        let cosh_mu = mu.cosh();
        let chebyshev_at_cutoff = if order % 2 == 0 {
            (T::one() / epsilon_squared + from_f64(2.0)).sqrt()
        } else {
            T::one() / epsilon
        };
        let cutoff_scale = (chebyshev_at_cutoff.acosh() / order_scalar).cosh();
        if !cutoff_scale.is_finite() || cutoff_scale <= T::zero() {
            return Err(BuildError::InvalidPassbandRipple);
        }

        let mut section = 0;
        if order % 2 == 1 {
            self.add_real_pole(section, sinh_mu / cutoff_scale, fractional_bandwidth);
            section += 1;
        }
        let denominator = from_f64::<T>(2.0) * order_scalar;
        for index in 0..order / 2 {
            let angle = T::PI() * from_usize::<T>(2 * index + 1) / denominator;
            let real = sinh_mu * angle.sin() / cutoff_scale;
            let imaginary = cosh_mu * angle.cos() / cutoff_scale;
            self.add_complex_pair(
                &mut section,
                from_f64::<T>(2.0) * real,
                real.mul_add(real, imaginary * imaginary),
                fractional_bandwidth,
            )?;
        }
        Ok(section)
    }

    fn add_real_pole(&mut self, section: usize, rate: T, fractional_bandwidth: T) {
        let coefficient = fractional_bandwidth * rate;
        self.damping[section] = coefficient;
        self.frequency_squared[section] = T::one();
        self.gain[section] = coefficient;
    }

    fn add_complex_pair(
        &mut self,
        section: &mut usize,
        low_pass_damping: T,
        low_pass_frequency_squared: T,
        fractional_bandwidth: T,
    ) -> Result<(), BuildError> {
        let two = from_f64::<T>(2.0);
        let four = from_f64::<T>(4.0);
        let bandwidth_squared = fractional_bandwidth * fractional_bandwidth;

        // Factoring the transformed fourth-order denominator through its
        // complex roots loses accuracy for narrow bands: each mapped root is
        // obtained by subtracting nearly equal complex numbers. Instead, use
        // the reciprocal symmetry of the two real band-pass sections.
        //
        // For a low-pass factor z² + a z + b, the band-pass substitution
        // z = (s² + 1) / (beta s) gives
        //
        // s⁴ + a beta s³ + (2 + b beta²) s² + a beta s + 1.
        //
        // Its real factors have natural frequencies W and 1/W and damping
        // coefficients c W and c/W. Solving for y = W + 1/W gives the
        // expressions below without constructing complex roots.
        let middle = four + low_pass_frequency_squared * bandwidth_squared;
        let discriminant = middle.mul_add(
            middle,
            -(four * low_pass_damping * low_pass_damping * bandwidth_squared),
        );
        if !discriminant.is_finite() || discriminant < T::zero() {
            return Err(BuildError::InvalidBandPassEdges);
        }

        let discriminant_root = discriminant.sqrt();
        // Compute y² - 4 directly. Forming y² first and then subtracting four
        // discards most of the useful bits when beta is small.
        let separation_squared = bandwidth_squared
            * (low_pass_frequency_squared
                + (from_f64::<T>(8.0) * low_pass_frequency_squared
                    - four * low_pass_damping * low_pass_damping
                    + low_pass_frequency_squared * low_pass_frequency_squared * bandwidth_squared)
                    / (discriminant_root + four))
            / two;
        if !separation_squared.is_finite() || separation_squared < T::zero() {
            return Err(BuildError::InvalidBandPassEdges);
        }
        let y = (four + separation_squared).sqrt();
        let separation = separation_squared.sqrt();
        let high_frequency = (y + separation) / two;
        let low_frequency = T::one() / high_frequency;
        let damping_scale = low_pass_damping * fractional_bandwidth / y;
        let section_gain = fractional_bandwidth * low_pass_frequency_squared.sqrt();

        self.damping[*section] = damping_scale * low_frequency;
        self.frequency_squared[*section] = low_frequency * low_frequency;
        self.gain[*section] = section_gain;
        *section += 1;
        self.damping[*section] = damping_scale * high_frequency;
        self.frequency_squared[*section] = high_frequency * high_frequency;
        self.gain[*section] = section_gain;
        *section += 1;
        Ok(())
    }

    fn is_valid(&mut self, section_count: usize) -> bool {
        for index in 0..section_count {
            if !self.damping[index].is_finite()
                || self.damping[index] <= T::zero()
                || !self.frequency_squared[index].is_finite()
                || self.frequency_squared[index] <= T::zero()
                || !self.gain[index].is_finite()
                || self.gain[index] <= T::zero()
            {
                return false;
            }

            let natural_frequency = self.frequency_squared[index].sqrt();
            let half_damping = self.damping[index] / from_f64(2.0);
            let pole_magnitude = if half_damping > natural_frequency {
                // The section is overdamped. This ratio form avoids squaring
                // a potentially large damping coefficient.
                let ratio = natural_frequency / half_damping;
                half_damping * (T::one() + (T::one() - ratio * ratio).max(T::zero()).sqrt())
            } else {
                natural_frequency
            };
            if !pole_magnitude.is_finite() {
                return false;
            }
            self.max_pole_magnitude = self.max_pole_magnitude.max(pole_magnitude);
        }
        self.max_pole_magnitude.is_finite() && self.max_pole_magnitude > T::zero()
    }

    fn sort_low_q_first(&mut self, section_count: usize) {
        for index in 1..section_count {
            let mut position = index;
            while position > 0
                && self.damping[position - 1] / self.frequency_squared[position - 1].sqrt()
                    < self.damping[position] / self.frequency_squared[position].sqrt()
            {
                self.damping.swap(position - 1, position);
                self.frequency_squared.swap(position - 1, position);
                self.gain.swap(position - 1, position);
                position -= 1;
            }
        }
    }

    #[cfg(test)]
    fn section_coefficients(&self) -> (&[T], &[T], &[T]) {
        (
            &self.damping[..N / 2],
            &self.frequency_squared[..N / 2],
            &self.gain[..N / 2],
        )
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for BandPassModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        for section in 0..N / 2 {
            let state_index = 2 * section;
            let position = state[state_index];
            let velocity = state[state_index + 1];
            derivative[state_index] = velocity;
            derivative[state_index + 1] = driving_signal
                - self.damping[section] * velocity
                - self.frequency_squared[section] * position;
            driving_signal = self.gain[section] * velocity;
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        if N == 0 {
            input
        } else {
            self.gain[N / 2 - 1] * state[N - 1]
        }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        let mut state = [T::zero(); N];
        if N >= 2 {
            state[0] = input / self.frequency_squared[0];
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

    use super::*;

    #[test]
    fn center_and_edges_have_the_expected_gain() {
        let lower = 0.5_f64;
        let upper = 2.0_f64;
        let fractional_bandwidth = upper - lower;
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            for magnitude in [
                magnitude::<8>(response, fractional_bandwidth, lower),
                magnitude::<8>(response, fractional_bandwidth, upper),
            ] {
                assert_relative_eq!(magnitude, 1.0 / 2.0_f64.sqrt(), epsilon = 3.0e-13);
            }
            assert_relative_eq!(
                magnitude::<8>(response, fractional_bandwidth, 1.0),
                1.0,
                epsilon = 3.0e-13
            );
        }
    }

    fn magnitude<const ORDER: usize>(
        response: Response,
        fractional_bandwidth: f64,
        frequency: f64,
    ) -> f64 {
        let model = BandPassModel::<f64, ORDER>::new(response, fractional_bandwidth).unwrap();
        let (damping, frequency_squared, gain) = model.section_coefficients();
        damping.iter().zip(frequency_squared).zip(gain).fold(
            1.0,
            |magnitude, ((&damping, &frequency_squared), &gain)| {
                magnitude * gain * frequency
                    / (frequency_squared - frequency * frequency).hypot(damping * frequency)
            },
        )
    }

    #[test]
    fn constant_input_is_an_exact_zero_output_equilibrium() {
        let model = BandPassModel::<f64, 6>::new(Response::Butterworth, 0.75).unwrap();
        let state = model.equilibrium(3.0);
        let mut derivative = [1.0; 6];
        model.derivative(&state, 3.0, &mut derivative);
        assert_eq!(derivative, [0.0; 6]);
        assert_eq!(model.output(&state, 3.0), 0.0);
    }

    #[test]
    fn wide_band_step_cap_tracks_the_fastest_overdamped_pole() {
        let model = BandPassModel::<f64, 2>::new(Response::Butterworth, 100.0).unwrap();
        let (damping, _, _) = model.section_coefficients();
        let half_damping = damping[0] / 2.0;
        let expected_fast_pole = half_damping + (half_damping * half_damping - 1.0).sqrt();

        assert_relative_eq!(
            1.0 / model.max_normalized_step(),
            expected_fast_pole,
            epsilon = 2.0e-14
        );
        assert!(model.max_normalized_step() < 0.011);
    }

    type BandwidthCheck = fn(Response, f64);

    #[rstest::rstest]
    #[case::order_2(check_bandwidth::<2>)]
    #[case::order_8(check_bandwidth::<8>)]
    #[case::order_50(check_bandwidth::<50>)]
    fn narrow_and_wide_bands_remain_normalized(
        #[case] check: BandwidthCheck,
        #[values(
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 }
        )]
        response: Response,
        #[values(0.001, 0.01, 0.1, 1.0, 10.0, 100.0)] fractional_bandwidth: f64,
    ) {
        check(response, fractional_bandwidth);
    }

    fn check_bandwidth<const ORDER: usize>(response: Response, fractional_bandwidth: f64) {
        let upper = fractional_bandwidth.hypot(2.0) / 2.0 + fractional_bandwidth / 2.0;
        let lower = 1.0 / upper;
        assert_relative_eq!(
            magnitude::<ORDER>(response, fractional_bandwidth, lower),
            1.0 / 2.0_f64.sqrt(),
            epsilon = 2.0e-10,
            max_relative = 2.0e-10
        );
        assert_relative_eq!(
            magnitude::<ORDER>(response, fractional_bandwidth, upper),
            1.0 / 2.0_f64.sqrt(),
            epsilon = 2.0e-10,
            max_relative = 2.0e-10
        );
    }
}
