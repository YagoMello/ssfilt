mod repeated_pole;

pub(crate) use repeated_pole::RepeatedPole;

use crate::Scalar;

pub(crate) trait ContinuousModel<T: Scalar, const N: usize> {
    fn derivative(&self, state: &[T; N], input: T, derivative: &mut [T; N]);
    fn output(&self, state: &[T; N], input: T) -> T;
    fn equilibrium(&self, input: T) -> [T; N];
    fn max_normalized_step(&self) -> T;
}
