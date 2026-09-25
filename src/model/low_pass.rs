use crate::{Response, Scalar};

use super::{Butterworth, ContinuousModel, RepeatedPole};

#[derive(Clone, Copy, Debug)]
pub(crate) enum LowPassModel<T, const N: usize> {
    RepeatedPole(RepeatedPole<T>),
    Butterworth(Butterworth<T, N>),
}

impl<T: Scalar, const N: usize> LowPassModel<T, N> {
    pub(crate) fn new(response: Response) -> Self {
        match response {
            Response::RepeatedPole => Self::RepeatedPole(RepeatedPole::new(N)),
            Response::Butterworth => Self::Butterworth(Butterworth::new()),
        }
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for LowPassModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        match self {
            Self::RepeatedPole(model) => model.derivative(state, input, derivative),
            Self::Butterworth(model) => model.derivative(state, input, derivative),
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        match self {
            Self::RepeatedPole(model) => model.output(state, input),
            Self::Butterworth(model) => model.output(state, input),
        }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        match self {
            Self::RepeatedPole(model) => model.equilibrium(input),
            Self::Butterworth(model) => model.equilibrium(input),
        }
    }

    fn max_normalized_step(&self) -> T {
        match self {
            Self::RepeatedPole(model) => {
                <RepeatedPole<T> as ContinuousModel<T, N>>::max_normalized_step(model)
            }
            Self::Butterworth(model) => model.max_normalized_step(),
        }
    }
}
