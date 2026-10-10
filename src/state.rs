use crate::Scalar;

/// Fixed-size state accessed by the private integration machinery.
///
/// Arrays are the ordinary representation. Composite models can implement
/// this trait with separate fixed arrays, avoiding unstable const expressions
/// such as `[T; N + 2]` without introducing allocation or a public ODE API.
pub(crate) trait StateVector<T: Scalar>: Copy {
    const LEN: usize;

    fn zeros() -> Self;
    fn component(&self, index: usize) -> T;
    fn set_component(&mut self, index: usize, value: T);

    fn all_finite(&self) -> bool {
        (0..Self::LEN).all(|index| self.component(index).is_finite())
    }
}

impl<T: Scalar, const N: usize> StateVector<T> for [T; N] {
    const LEN: usize = N;

    fn zeros() -> Self {
        [T::zero(); N]
    }

    fn component(&self, index: usize) -> T {
        self[index]
    }

    fn set_component(&mut self, index: usize, value: T) {
        self[index] = value;
    }
}
