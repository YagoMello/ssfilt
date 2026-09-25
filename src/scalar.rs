use core::fmt::Debug;

use num_traits::{Float, FloatConst};

mod sealed {
    pub trait Sealed {}

    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// Floating-point scalar types validated and supported by `ssfilt`.
///
/// This trait is sealed. Supporting a scalar means validating the complete
/// numerical behavior of the filter, so the initial API deliberately supports
/// only `f32` and `f64`.
pub trait Scalar: sealed::Sealed + Float + FloatConst + Debug {}

impl Scalar for f32 {}
impl Scalar for f64 {}

pub(crate) fn from_usize<T: Scalar>(value: usize) -> T {
    T::from(value).expect("usize used by ssfilt must fit in f32 and f64")
}

pub(crate) fn from_f64<T: Scalar>(value: f64) -> T {
    T::from(value).expect("finite f64 constant must fit in f32 and f64")
}
