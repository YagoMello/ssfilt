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
                if fraction == T::zero() {
                    return self.start;
                }
                if fraction == T::one() {
                    return self.end;
                }

                // A convex combination avoids overflowing `end - start` when
                // finite endpoints have opposite signs and large magnitudes.
                (T::one() - fraction) * self.start + fraction * self.end
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
    fn linear_interpolation_handles_opposite_f32_max_endpoints() {
        let linear = InputSegment::new(-f32::MAX, f32::MAX, InputModel::Linear);

        assert_eq!(linear.value_at(0.0), -f32::MAX);
        assert_eq!(linear.value_at(1.0), f32::MAX);
        assert_eq!(linear.value_at(0.5), 0.0);
        assert!(linear.value_at(0.25).is_finite());
        assert!(linear.value_at(0.75).is_finite());

        let ordinary = InputSegment::new(2.0_f32, 6.0, InputModel::Linear);
        assert_eq!(ordinary.value_at(0.25), 3.0);
    }

    #[test]
    fn linear_interpolation_handles_opposite_f64_max_endpoints() {
        let linear = InputSegment::new(-f64::MAX, f64::MAX, InputModel::Linear);

        assert_eq!(linear.value_at(0.0), -f64::MAX);
        assert_eq!(linear.value_at(1.0), f64::MAX);
        assert_eq!(linear.value_at(0.5), 0.0);
        assert!(linear.value_at(0.25).is_finite());
        assert!(linear.value_at(0.75).is_finite());

        let ordinary = InputSegment::new(2.0_f64, 6.0, InputModel::Linear);
        assert_eq!(ordinary.value_at(0.25), 3.0);
    }
}
