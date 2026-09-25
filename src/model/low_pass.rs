use crate::scalar::from_usize;
use crate::{BuildError, Response, Scalar};

use super::{Bessel, Butterworth, Chebyshev1, ContinuousModel, RepeatedPole};

#[derive(Clone, Copy, Debug)]
pub(crate) enum LowPassModel<T, const N: usize> {
    RepeatedPole(RepeatedPole<T>),
    Butterworth(Butterworth<T, N>),
    Bessel(Bessel<T, N>),
    Chebyshev1(Chebyshev1<T, N>),
}

impl<T: Scalar, const N: usize> LowPassModel<T, N> {
    pub(crate) fn new(response: Response<T>) -> Result<Self, BuildError> {
        match response {
            Response::RepeatedPole => Ok(Self::RepeatedPole(RepeatedPole::new(N))),
            Response::Butterworth => Ok(Self::Butterworth(Butterworth::new())),
            Response::Bessel => Bessel::new()
                .map(Self::Bessel)
                .ok_or(BuildError::UnsupportedBesselOrder),
            Response::Chebyshev1 { ripple_db } => Chebyshev1::new(ripple_db)
                .map(Self::Chebyshev1)
                .ok_or(BuildError::InvalidPassbandRipple),
        }
    }

    /// Group delay in normalized time at a nonnegative normalized frequency.
    pub(crate) fn group_delay(&self, frequency: T) -> T {
        let frequency_squared = frequency * frequency;
        match self {
            Self::RepeatedPole(model) => {
                let rate = model.rate();
                from_usize::<T>(N) * rate / (rate * rate + frequency_squared)
            }
            Self::Butterworth(model) => {
                let mut delay = if N % 2 == 1 {
                    T::one() / (T::one() + frequency_squared)
                } else {
                    T::zero()
                };
                for &damping in model.damping() {
                    delay = delay + biquad_delay(damping, T::one(), frequency_squared);
                }
                delay
            }
            Self::Bessel(model) => {
                let mut delay = if N % 2 == 1 {
                    real_pole_delay(model.real_rate(), frequency_squared)
                } else {
                    T::zero()
                };
                let (damping, section_frequencies_squared) = model.section_coefficients();
                for (&damping, &frequency_square) in damping.iter().zip(section_frequencies_squared)
                {
                    delay = delay + biquad_delay(damping, frequency_square, frequency_squared);
                }
                delay
            }
            Self::Chebyshev1(model) => {
                let mut delay = if N % 2 == 1 {
                    real_pole_delay(model.real_rate(), frequency_squared)
                } else {
                    T::zero()
                };
                let (damping, section_frequencies_squared) = model.section_coefficients();
                for (&damping, &frequency_square) in damping.iter().zip(section_frequencies_squared)
                {
                    delay = delay + biquad_delay(damping, frequency_square, frequency_squared);
                }
                delay
            }
        }
    }
}

fn real_pole_delay<T: Scalar>(rate: T, frequency_squared: T) -> T {
    rate / (rate * rate + frequency_squared)
}

fn biquad_delay<T: Scalar>(damping: T, natural_frequency_squared: T, frequency_squared: T) -> T {
    let real = natural_frequency_squared - frequency_squared;
    let imaginary_squared = damping * damping * frequency_squared;
    damping * (natural_frequency_squared + frequency_squared) / (real * real + imaginary_squared)
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for LowPassModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        match self {
            Self::RepeatedPole(model) => model.derivative(state, input, derivative),
            Self::Butterworth(model) => model.derivative(state, input, derivative),
            Self::Bessel(model) => model.derivative(state, input, derivative),
            Self::Chebyshev1(model) => model.derivative(state, input, derivative),
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        match self {
            Self::RepeatedPole(model) => model.output(state, input),
            Self::Butterworth(model) => model.output(state, input),
            Self::Bessel(model) => model.output(state, input),
            Self::Chebyshev1(model) => model.output(state, input),
        }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        match self {
            Self::RepeatedPole(model) => model.equilibrium(input),
            Self::Butterworth(model) => model.equilibrium(input),
            Self::Bessel(model) => model.equilibrium(input),
            Self::Chebyshev1(model) => model.equilibrium(input),
        }
    }

    fn max_normalized_step(&self) -> T {
        match self {
            Self::RepeatedPole(model) => {
                <RepeatedPole<T> as ContinuousModel<T, N>>::max_normalized_step(model)
            }
            Self::Butterworth(model) => model.max_normalized_step(),
            Self::Bessel(model) => model.max_normalized_step(),
            Self::Chebyshev1(model) => model.max_normalized_step(),
        }
    }
}

#[cfg(test)]
mod group_delay_tests {
    use approx::assert_relative_eq;

    use super::*;

    #[test]
    fn first_order_delay_matches_analytic_pole() {
        let model = LowPassModel::<f64, 1>::new(Response::Butterworth).unwrap();
        assert_relative_eq!(model.group_delay(0.0), 1.0, epsilon = 1.0e-15);
        assert_relative_eq!(model.group_delay(1.0), 0.5, epsilon = 1.0e-15);
    }

    #[test]
    fn second_order_butterworth_has_known_cutoff_delay() {
        let model = LowPassModel::<f64, 2>::new(Response::Butterworth).unwrap();
        assert_relative_eq!(model.group_delay(1.0), 2.0_f64.sqrt(), epsilon = 1.0e-14);
    }
}
