use crate::{Response, Scalar};

use super::{Butterworth, Chebyshev1, ContinuousModel, RepeatedPole};

#[derive(Clone, Copy, Debug)]
pub(crate) enum LowPassModel<T, const N: usize> {
    RepeatedPole(RepeatedPole<T>),
    Butterworth(Butterworth<T, N>),
    Chebyshev1(Chebyshev1<T, N>),
}

impl<T: Scalar, const N: usize> LowPassModel<T, N> {
    pub(crate) fn new(response: Response<T>) -> Option<Self> {
        match response {
            Response::RepeatedPole => Some(Self::RepeatedPole(RepeatedPole::new(N))),
            Response::Butterworth => Some(Self::Butterworth(Butterworth::new())),
            Response::Chebyshev1 { ripple_db } => Chebyshev1::new(ripple_db).map(Self::Chebyshev1),
        }
    }
}

impl<T: Scalar, const N: usize> ContinuousModel<T, N> for LowPassModel<T, N> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]) {
        match self {
            Self::RepeatedPole(model) => model.derivative(state, input, derivative),
            Self::Butterworth(model) => model.derivative(state, input, derivative),
            Self::Chebyshev1(model) => model.derivative(state, input, derivative),
        }
    }

    fn output(&self, state: &[T; N], input: T) -> T {
        match self {
            Self::RepeatedPole(model) => model.output(state, input),
            Self::Butterworth(model) => model.output(state, input),
            Self::Chebyshev1(model) => model.output(state, input),
        }
    }

    fn equilibrium(&self, input: T) -> [T; N] {
        match self {
            Self::RepeatedPole(model) => model.equilibrium(input),
            Self::Butterworth(model) => model.equilibrium(input),
            Self::Chebyshev1(model) => model.equilibrium(input),
        }
    }

    fn max_normalized_step(&self) -> T {
        match self {
            Self::RepeatedPole(model) => {
                <RepeatedPole<T> as ContinuousModel<T, N>>::max_normalized_step(model)
            }
            Self::Butterworth(model) => model.max_normalized_step(),
            Self::Chebyshev1(model) => model.max_normalized_step(),
        }
    }
}
