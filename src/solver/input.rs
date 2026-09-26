use crate::{InputModel, Scalar};

#[derive(Clone, Copy, Debug)]
pub(crate) struct InputSegment<T> {
    start: T,
    end: T,
    model: InputModel,
}

impl<T: Scalar> InputSegment<T> {
    pub(crate) const fn new(start: T, end: T, model: InputModel) -> Self {
        Self { start, end, model }
    }

    pub(crate) fn value_at(self, fraction: T) -> T {
        match self.model {
            InputModel::Linear => {
                if self.start.is_sign_positive() == self.end.is_sign_positive() {
                    self.start + fraction * (self.end - self.start)
                } else {
                    // Opposite-sign endpoints can have an infinite difference even
                    // when every interpolated value is finite.
                    (T::one() - fraction) * self.start + fraction * self.end
                }
            }
            InputModel::PreviousHold => self.start,
            InputModel::CurrentHold => self.end,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn interpolation_policies_have_explicit_endpoint_behavior() {
        let linear = InputSegment::new(2.0, 6.0, InputModel::Linear);
        assert_eq!(linear.value_at(0.0), 2.0);
        assert_eq!(linear.value_at(0.25), 3.0);
        assert_eq!(linear.value_at(1.0), 6.0);

        let previous = InputSegment::new(2.0, 6.0, InputModel::PreviousHold);
        assert_eq!(previous.value_at(1.0), 2.0);

        let current = InputSegment::new(2.0, 6.0, InputModel::CurrentHold);
        assert_eq!(current.value_at(0.0), 6.0);
    }

    #[test]
    fn linear_interpolation_stays_finite_across_extreme_opposite_signs() {
        for (start, end) in [(-f64::MAX, f64::MAX), (f64::MAX, -f64::MAX)] {
            let segment = InputSegment::new(start, end, InputModel::Linear);
            assert_eq!(segment.value_at(0.0), start);
            assert_eq!(segment.value_at(0.5), 0.0);
            assert_eq!(segment.value_at(1.0), end);
            assert!(segment.value_at(0.25).is_finite());
            assert!(segment.value_at(0.75).is_finite());
        }

        for (start, end) in [(-f32::MAX, f32::MAX), (f32::MAX, -f32::MAX)] {
            let segment = InputSegment::new(start, end, InputModel::Linear);
            assert_eq!(segment.value_at(0.0), start);
            assert_eq!(segment.value_at(0.5), 0.0);
            assert_eq!(segment.value_at(1.0), end);
            assert!(segment.value_at(0.25).is_finite());
            assert!(segment.value_at(0.75).is_finite());
        }
    }
}
