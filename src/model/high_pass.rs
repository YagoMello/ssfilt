use crate::{BuildError, Response, Scalar};

use super::{Bessel, Butterworth, Chebyshev1, ContinuousModel, RepeatedPole};

#[derive(Clone, Copy, Debug)]
pub(crate) enum HighPassModel<T, const N: usize> {
    RepeatedPole(RepeatedPoleHighPass<T>),
    Classical(ClassicalHighPass<T, N>),
}

impl<T: Scalar, const N: usize> HighPassModel<T, N> {
    pub(crate) fn new(response: Response<T>) -> Result<Self, BuildError> {
        match response {
            Response::RepeatedPole => {
                let low_pass_rate = RepeatedPole::new(N).rate();
                Ok(Self::RepeatedPole(RepeatedPoleHighPass {
                    rate: T::one() / low_pass_rate,
                }))
            }
            Response::Butterworth => {
                let prototype = Butterworth::<T, N>::new();
                Ok(Self::Classical(ClassicalHighPass::from_low_pass(
                    T::one(),
                    prototype.damping(),
                    None,
                )))
            }
            Response::Bessel => {
                let prototype = Bessel::<T, N>::new().ok_or(BuildError::UnsupportedBesselOrder)?;
                let (damping, frequency_squared) = prototype.section_coefficients();
                Ok(Self::Classical(ClassicalHighPass::from_low_pass(
                    prototype.real_rate(),
                    damping,
                    Some(frequency_squared),
                )))
            }
            Response::Chebyshev1 { ripple_db } => {
                let prototype =
                    Chebyshev1::<T, N>::new(ripple_db).ok_or(BuildError::InvalidPassbandRipple)?;
                let (damping, frequency_squared) = prototype.section_coefficients();
                Ok(Self::Classical(ClassicalHighPass::from_low_pass(
                    prototype.real_rate(),
                    damping,
                    Some(frequency_squared),
                )))
            }
        }
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for HighPassModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        match self {
            Self::RepeatedPole(model) => model.derivative(state, input, derivative),
            Self::Classical(model) => model.derivative(state, input, derivative),
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        match self {
            Self::RepeatedPole(model) => model.output(state, input),
            Self::Classical(model) => model.output(state, input),
        }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        match self {
            Self::RepeatedPole(model) => model.equilibrium(input),
            Self::Classical(model) => model.equilibrium(input),
        }
    }

    fn max_normalized_step(&self) -> T {
        match self {
            Self::RepeatedPole(model) => {
                <RepeatedPoleHighPass<T> as ContinuousModel<T, N>>::max_normalized_step(model)
            }
            Self::Classical(model) => model.max_normalized_step(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RepeatedPoleHighPass<T> {
    rate: T,
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for RepeatedPoleHighPass<T> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        for index in 0..N {
            let output = driving_signal - state[index];
            derivative[index] = self.rate * output;
            driving_signal = output;
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        state.iter().fold(input, |signal, state| signal - *state)
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        let mut state = [T::zero(); N];
        if N > 0 {
            state[0] = input;
        }
        state
    }

    fn max_normalized_step(&self) -> T {
        T::one() / self.rate
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ClassicalHighPass<T, const N: usize> {
    real_rate: T,
    damping: [T; N],
    frequency_squared: [T; N],
    max_pole_magnitude: T,
}

impl<T: Scalar, const N: usize> ClassicalHighPass<T, N> {
    fn from_low_pass(
        low_pass_real_rate: T,
        low_pass_damping: &[T],
        low_pass_frequency_squared: Option<&[T]>,
    ) -> Self {
        let mut damping = [T::zero(); N];
        let mut frequency_squared = [T::zero(); N];
        let mut max_pole_magnitude = T::zero();

        for (index, &low_damping) in low_pass_damping.iter().enumerate() {
            let low_frequency_squared =
                low_pass_frequency_squared.map_or(T::one(), |values| values[index]);
            damping[index] = low_damping / low_frequency_squared;
            frequency_squared[index] = T::one() / low_frequency_squared;
            max_pole_magnitude = max_pole_magnitude.max(frequency_squared[index].sqrt());
        }

        let real_rate = if N % 2 == 1 {
            T::one() / low_pass_real_rate
        } else {
            T::zero()
        };
        max_pole_magnitude = max_pole_magnitude.max(real_rate);

        Self {
            real_rate,
            damping,
            frequency_squared,
            max_pole_magnitude,
        }
    }

    fn section_output(&self, section: usize, input: T, position: T, velocity: T) -> T {
        input - position - self.damping[section] / self.frequency_squared[section] * velocity
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for ClassicalHighPass<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        let mut driving_signal = input;
        let mut state_index = 0;

        if N % 2 == 1 {
            derivative[0] = self.real_rate * (driving_signal - state[0]);
            driving_signal = driving_signal - state[0];
            state_index = 1;
        }

        let mut section_index = 0;
        while state_index + 1 < N {
            let position = state[state_index];
            let velocity = state[state_index + 1];
            derivative[state_index] = velocity;
            derivative[state_index + 1] = self.frequency_squared[section_index]
                * (driving_signal - position)
                - self.damping[section_index] * velocity;
            driving_signal = self.section_output(section_index, driving_signal, position, velocity);
            state_index += 2;
            section_index += 1;
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        let mut signal = input;
        let mut state_index = 0;
        if N % 2 == 1 {
            signal = signal - state[0];
            state_index = 1;
        }
        let mut section_index = 0;
        while state_index + 1 < N {
            signal = self.section_output(
                section_index,
                signal,
                state[state_index],
                state[state_index + 1],
            );
            state_index += 2;
            section_index += 1;
        }
        signal
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        let mut state = [T::zero(); N];
        if N > 0 {
            state[0] = input;
        }
        state
    }

    fn max_normalized_step(&self) -> T {
        T::one() / self.max_pole_magnitude
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

    use super::*;

    #[test]
    fn steady_input_has_zero_output_and_derivative() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let model = HighPassModel::<f64, 5>::new(response).unwrap();
            let state = model.equilibrium(2.5);
            let mut derivative = [1.0; 5];
            model.derivative(&state, 2.5, &mut derivative);
            assert_eq!(derivative, [0.0; 5]);
            assert_eq!(model.output(&state, 2.5), 0.0);
        }
    }

    #[test]
    fn first_order_step_has_exact_exponential_decay() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let model = HighPassModel::<f64, 1>::new(response).unwrap();
            let state = [0.25];
            assert_relative_eq!(model.output(&state, 1.0), 0.75, epsilon = 1.0e-15);
        }
    }

    #[test]
    fn transformed_models_share_the_minus_three_db_cutoff() {
        for order in 1..=16 {
            for response in [
                Response::RepeatedPole,
                Response::Butterworth,
                Response::Bessel,
                Response::Chebyshev1 { ripple_db: 0.5 },
            ] {
                if response == Response::Bessel && order > crate::MAX_BESSEL_ORDER {
                    continue;
                }
                let magnitude = match order {
                    1 => cutoff_magnitude::<1>(response),
                    2 => cutoff_magnitude::<2>(response),
                    3 => cutoff_magnitude::<3>(response),
                    4 => cutoff_magnitude::<4>(response),
                    5 => cutoff_magnitude::<5>(response),
                    6 => cutoff_magnitude::<6>(response),
                    7 => cutoff_magnitude::<7>(response),
                    8 => cutoff_magnitude::<8>(response),
                    9 => cutoff_magnitude::<9>(response),
                    10 => cutoff_magnitude::<10>(response),
                    11 => cutoff_magnitude::<11>(response),
                    12 => cutoff_magnitude::<12>(response),
                    13 => cutoff_magnitude::<13>(response),
                    14 => cutoff_magnitude::<14>(response),
                    15 => cutoff_magnitude::<15>(response),
                    16 => cutoff_magnitude::<16>(response),
                    _ => unreachable!(),
                };
                assert_relative_eq!(
                    magnitude,
                    1.0 / 2.0_f64.sqrt(),
                    epsilon = 2.0e-13,
                    max_relative = 2.0e-13
                );
            }
        }
    }

    fn cutoff_magnitude<const N: usize>(response: Response) -> f64 {
        match HighPassModel::<f64, N>::new(response).unwrap() {
            HighPassModel::RepeatedPole(model) => (1.0 / model.rate.hypot(1.0)).powi(N as i32),
            HighPassModel::Classical(model) => {
                let mut magnitude = if N % 2 == 1 {
                    1.0 / model.real_rate.hypot(1.0)
                } else {
                    1.0
                };
                for index in 0..N / 2 {
                    magnitude /= (model.frequency_squared[index] - 1.0).hypot(model.damping[index]);
                }
                magnitude
            }
        }
    }
}
