use core::fmt::Debug;

use num_traits::{Float, FloatConst};

mod sealed {
    pub trait Sealed {}

    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// Floating-point types supported by `ssfilt`.
///
/// This trait is sealed. The initial API deliberately supports `f32` and `f64`
/// without promising that arbitrary numeric types behave correctly.
pub trait Real: sealed::Sealed + Float + FloatConst + Debug {
    #[doc(hidden)]
    fn default_absolute_tolerance() -> Self;

    #[doc(hidden)]
    fn default_relative_tolerance() -> Self;
}

impl Real for f32 {
    fn default_absolute_tolerance() -> Self {
        1.0e-6
    }

    fn default_relative_tolerance() -> Self {
        1.0e-4
    }
}

impl Real for f64 {
    fn default_absolute_tolerance() -> Self {
        1.0e-10
    }

    fn default_relative_tolerance() -> Self {
        1.0e-7
    }
}

pub(crate) fn from_usize<T: Real>(value: usize) -> T {
    T::from(value).expect("usize used by ssfilt must fit in f32 and f64")
}

pub(crate) fn from_f64<T: Real>(value: f64) -> T {
    T::from(value).expect("finite f64 constant must fit in f32 and f64")
}
