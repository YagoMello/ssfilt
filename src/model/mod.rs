mod band_pass;
mod bessel;
mod bessel_table;
mod butterworth;
mod chebyshev1;
mod high_pass;
mod low_pass;
#[cfg(test)]
mod phase_equalized;
mod repeated_pole;

pub(crate) use butterworth::Butterworth;
pub(crate) use chebyshev1::Chebyshev1;
pub(crate) use high_pass::HighPassModel;
pub(crate) use low_pass::LowPassModel;
pub(crate) use repeated_pole::RepeatedPole;

use crate::Scalar;
use crate::state::StateVector;

pub(crate) trait ContinuousModel<T: Scalar, S: StateVector<T>> {
    fn derivative(&self, state: &S, input: T, derivative: &mut S);
    fn output(&self, state: &S, input: T) -> T;
    fn equilibrium(&self, input: T) -> S;
    fn max_normalized_step(&self) -> T;
}
pub(crate) use band_pass::BandPassModel;
pub(crate) use bessel::Bessel;
