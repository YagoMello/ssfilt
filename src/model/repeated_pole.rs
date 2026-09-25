use crate::Scalar;
use crate::model::ContinuousModel;
use crate::scalar::from_usize;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RepeatedPole<T> {
    rate: T,
}

impl<T: Scalar> RepeatedPole<T> {
    pub(crate) fn new(order: usize) -> Self {
        // exp_m1(ln(2) / N) avoids cancellation as N grows.
        let denominator = (T::LN_2() / from_usize::<T>(order)).exp_m1().sqrt();
        Self {
            rate: T::one() / denominator,
        }
    }

    #[cfg(test)]
    pub(crate) fn rate(self) -> T {
        self.rate
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for RepeatedPole<T> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        for index in 0..N {
            derivative[index] = self.rate * (driving_signal - state[index]);
            driving_signal = state[index];
        }
    }

    fn output(&self, state: &[T; N], _input: T) -> T {
        state.last().copied().unwrap_or_else(T::zero)
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        [input; N]
    }

    fn max_normalized_step(&self) -> T {
        T::one() / self.rate
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::float_cmp
    )]

    use approx::assert_relative_eq;

    use super::*;

    #[test]
    fn pole_rate_normalizes_total_response_to_minus_three_db() {
        for order in 1..=16 {
            let model = RepeatedPole::<f64>::new(order);
            let rate = model.rate();
            let stage_magnitude = rate / rate.hypot(1.0);
            let total_magnitude = stage_magnitude.powi(order as i32);
            assert_relative_eq!(total_magnitude, 1.0 / 2.0_f64.sqrt(), epsilon = 1e-14);
        }
    }

    #[test]
    fn equilibrium_has_zero_derivative() {
        let model = RepeatedPole::<f64>::new(4);
        let state = <RepeatedPole<f64> as ContinuousModel<f64, 4>>::equilibrium(&model, 3.25);
        let mut derivative = [1.0; 4];
        model.derivative(&state, 3.25, &mut derivative);
        assert_eq!(derivative, [0.0; 4]);
        assert_eq!(model.output(&state, 3.25), 3.25);
    }
}
