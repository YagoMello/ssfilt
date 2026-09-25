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
            InputModel::Linear => self.start + fraction * (self.end - self.start),
            InputModel::PreviousHold => self.start,
            InputModel::CurrentHold => self.end,
        }
    }

    pub(crate) fn is_constant(self) -> bool {
        self.start == self.end
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
}
