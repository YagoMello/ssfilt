use core::fmt;

use rug::{Float, float::Constant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordScalar {
    F32,
    F64,
}

impl RecordScalar {
    const fn scale_floor(self) -> f64 {
        match self {
            Self::F32 => 1.0e-2,
            Self::F64 => 1.0e-3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordError {
    InvalidCutoff,
    ShapeMismatch,
    NonFiniteSample { index: usize },
    InvalidInterval { index: usize },
}

impl fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCutoff => formatter.write_str("cutoff must be finite and positive"),
            Self::ShapeMismatch => formatter
                .write_str("a represented record needs one sample more than its interval count"),
            Self::NonFiniteSample { index } => {
                write!(formatter, "sample {index} is not finite")
            }
            Self::InvalidInterval { index } => {
                write!(formatter, "interval {index} must be finite and positive")
            }
        }
    }
}

impl std::error::Error for RecordError {}

/// Exact binary scalar values supplied to one candidate cell.
///
/// `f32` values are widened to `f64` only after their original rounding; that
/// conversion is exact and makes one storage shape sufficient for both scalar
/// cases.
#[derive(Clone, Debug)]
pub struct RepresentedRecord {
    scalar: RecordScalar,
    cutoff_hz: f64,
    intervals_seconds: Vec<f64>,
    samples: Vec<f64>,
}

impl RepresentedRecord {
    pub fn from_f64(
        cutoff_hz: f64,
        intervals_seconds: Vec<f64>,
        samples: Vec<f64>,
    ) -> Result<Self, RecordError> {
        Self::new(RecordScalar::F64, cutoff_hz, intervals_seconds, samples)
    }

    pub fn from_f32(
        cutoff_hz: f32,
        intervals_seconds: Vec<f32>,
        samples: Vec<f32>,
    ) -> Result<Self, RecordError> {
        Self::new(
            RecordScalar::F32,
            f64::from(cutoff_hz),
            intervals_seconds.into_iter().map(f64::from).collect(),
            samples.into_iter().map(f64::from).collect(),
        )
    }

    fn new(
        scalar: RecordScalar,
        cutoff_hz: f64,
        intervals_seconds: Vec<f64>,
        samples: Vec<f64>,
    ) -> Result<Self, RecordError> {
        if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
            return Err(RecordError::InvalidCutoff);
        }
        if samples.is_empty() || intervals_seconds.len() + 1 != samples.len() {
            return Err(RecordError::ShapeMismatch);
        }
        if let Some(index) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(RecordError::NonFiniteSample { index });
        }
        if let Some(index) = intervals_seconds
            .iter()
            .position(|interval| !interval.is_finite() || *interval <= 0.0)
        {
            return Err(RecordError::InvalidInterval { index });
        }

        Ok(Self {
            scalar,
            cutoff_hz,
            intervals_seconds,
            samples,
        })
    }

    #[must_use]
    pub const fn scalar(&self) -> RecordScalar {
        self.scalar
    }

    #[must_use]
    pub const fn cutoff_hz(&self) -> f64 {
        self.cutoff_hz
    }

    #[must_use]
    pub fn intervals_seconds(&self) -> &[f64] {
        &self.intervals_seconds
    }

    #[must_use]
    pub fn samples(&self) -> &[f64] {
        &self.samples
    }

    #[must_use]
    pub fn scale(&self, precision: u32) -> Float {
        let maximum = self
            .samples
            .iter()
            .copied()
            .map(f64::abs)
            .fold(self.scalar.scale_floor(), f64::max);
        Float::with_val(precision, maximum)
    }

    #[must_use]
    pub(crate) fn normalize(&self, precision: u32) -> NormalizedRecord {
        let mut angular_cutoff = Float::with_val(precision, Constant::Pi);
        angular_cutoff *= 2;
        angular_cutoff *= Float::with_val(precision, self.cutoff_hz);

        let intervals = self
            .intervals_seconds
            .iter()
            .map(|interval| {
                let mut normalized = Float::with_val(precision, *interval);
                normalized *= &angular_cutoff;
                normalized
            })
            .collect();
        let samples = self
            .samples
            .iter()
            .map(|sample| Float::with_val(precision, *sample))
            .collect();

        NormalizedRecord { intervals, samples }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedRecord {
    pub intervals: Vec<Float>,
    pub samples: Vec<Float>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_identifies_the_first_bad_element() {
        assert_eq!(
            RepresentedRecord::from_f64(0.0, vec![], vec![0.0]).unwrap_err(),
            RecordError::InvalidCutoff
        );
        assert_eq!(
            RepresentedRecord::from_f64(1.0, vec![1.0], vec![0.0]).unwrap_err(),
            RecordError::ShapeMismatch
        );
        assert_eq!(
            RepresentedRecord::from_f64(1.0, vec![1.0], vec![0.0, f64::NAN]).unwrap_err(),
            RecordError::NonFiniteSample { index: 1 }
        );
        assert_eq!(
            RepresentedRecord::from_f64(1.0, vec![0.0], vec![0.0, 1.0]).unwrap_err(),
            RecordError::InvalidInterval { index: 0 }
        );
    }

    #[test]
    fn f32_values_are_promoted_after_rounding() {
        let cutoff = 37.0_f32;
        let interval = 0.1_f32;
        let sample = 0.1_f32;
        let record =
            RepresentedRecord::from_f32(cutoff, vec![interval], vec![sample, -sample]).unwrap();

        assert_eq!(record.cutoff_hz(), f64::from(cutoff));
        assert_eq!(record.intervals_seconds(), &[f64::from(interval)]);
        assert_eq!(record.samples(), &[f64::from(sample), -f64::from(sample)]);
        assert_eq!(
            record.scale(192).to_f64(),
            1.0e-2_f64.max(f64::from(sample))
        );
    }

    #[test]
    fn normalization_uses_high_precision_mathematical_tau() {
        let record = RepresentedRecord::from_f64(1.0, vec![0.25], vec![0.0, 1.0]).unwrap();
        let normalized = record.normalize(192);
        let mut expected = Float::with_val(192, Constant::Pi);
        expected /= 2;

        let error = Float::with_val(192, &normalized.intervals[0] - expected).abs();
        assert!(error < Float::with_val(192, 1.0e-55));
    }
}
