//! Candidate-scalar one-third/two-thirds refinement of represented records.
//!
//! The elapsed split is performed in the represented scalar itself. Inserted
//! samples are then evaluated from the represented segment at 256-bit
//! precision and rounded once back to that scalar.

use core::fmt;

use rug::{Float, float::Round};

use crate::{
    FixtureCoordinate, PhysicalCutoff, RecordError, RecordScalar, RepresentedCase,
    RepresentedRecord, ScalarBits,
};

const REFINEMENT_PRECISION: u32 = 256;

/// Which output allocation could not reserve its checked required capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinedBuffer {
    Intervals,
    Samples,
}

/// A value stored in a represented record that failed exact `f32` narrowing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepresentedValueField {
    Cutoff,
    Interval,
    Sample,
}

/// The role of a value checked while splitting one represented interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitPart {
    ThirdDuration,
    TwoThirdsPosition,
    RemainderDuration,
}

impl fmt::Display for SplitPart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThirdDuration => formatter.write_str("third duration"),
            Self::TwoThirdsPosition => formatter.write_str("two-thirds position"),
            Self::RemainderDuration => formatter.write_str("remainder duration"),
        }
    }
}

/// The refined scalar record and the metadata of its unrefined source.
#[derive(Clone, Debug)]
pub struct ScalarRefinement {
    coordinate: FixtureCoordinate,
    physical_cutoff: PhysicalCutoff,
    candidate_cutoff_bits: ScalarBits,
    candidate_angular_cutoff_bits: ScalarBits,
    record: RepresentedRecord,
    original_sample_count: usize,
}

impl ScalarRefinement {
    #[must_use]
    pub const fn coordinate(&self) -> FixtureCoordinate {
        self.coordinate
    }

    #[must_use]
    pub const fn physical_cutoff(&self) -> PhysicalCutoff {
        self.physical_cutoff
    }

    /// Exact scalar bits of the source case's candidate cutoff.
    #[must_use]
    pub const fn candidate_cutoff_bits(&self) -> ScalarBits {
        self.candidate_cutoff_bits
    }

    /// Exact scalar bits of the source case's candidate angular cutoff.
    #[must_use]
    pub const fn candidate_angular_cutoff_bits(&self) -> ScalarBits {
        self.candidate_angular_cutoff_bits
    }

    #[must_use]
    pub const fn record(&self) -> &RepresentedRecord {
        &self.record
    }

    #[must_use]
    pub const fn original_sample_count(&self) -> usize {
        self.original_sample_count
    }

    /// Return the refined sample index for one original sample index.
    #[must_use]
    pub fn refined_index_for_original(&self, original_index: usize) -> Option<usize> {
        let refined_index = original_index.checked_mul(3)?;
        (original_index < self.original_sample_count && refined_index < self.record.samples().len())
            .then_some(refined_index)
    }
}

/// Contextual failure while constructing one candidate-scalar refinement.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarRefinementError {
    coordinate: FixtureCoordinate,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
    kind: ScalarRefinementErrorKind,
}

impl ScalarRefinementError {
    #[must_use]
    pub const fn coordinate(&self) -> FixtureCoordinate {
        self.coordinate
    }

    #[must_use]
    pub const fn scalar(&self) -> RecordScalar {
        self.scalar
    }

    #[must_use]
    pub const fn cutoff(&self) -> PhysicalCutoff {
        self.cutoff
    }

    #[must_use]
    pub const fn kind(&self) -> &ScalarRefinementErrorKind {
        &self.kind
    }
}

/// The cause of a scalar-refinement failure, with record indices kept separate
/// from its fixture, scalar, and cutoff context.
#[derive(Clone, Debug, PartialEq)]
pub enum ScalarRefinementErrorKind {
    RefinedLengthOverflow {
        interval_count: usize,
    },
    CapacityReservationFailed {
        buffer: RefinedBuffer,
        requested_capacity: usize,
    },
    NonExactF32Storage {
        field: RepresentedValueField,
        index: Option<usize>,
        value: f64,
    },
    InvalidSplitValue {
        interval_index: usize,
        part: SplitPart,
        value: f64,
    },
    DurationConservationFailure {
        interval_index: usize,
    },
    NonFiniteInsertedSample {
        interval_index: usize,
        inserted_index: usize,
        value: f64,
    },
    RecordValidation(RecordError),
}

impl fmt::Display for ScalarRefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "scalar refinement failed for {:?}, {:?}, {:?}: ",
            self.coordinate, self.scalar, self.cutoff
        )?;
        match &self.kind {
            ScalarRefinementErrorKind::RefinedLengthOverflow { interval_count } => write!(
                formatter,
                "refining {interval_count} intervals overflows the refined record length"
            ),
            ScalarRefinementErrorKind::CapacityReservationFailed {
                buffer,
                requested_capacity,
            } => write!(
                formatter,
                "could not reserve {requested_capacity} entries for the {buffer:?} buffer"
            ),
            ScalarRefinementErrorKind::NonExactF32Storage {
                field,
                index,
                value,
            } => match index {
                Some(index) => write!(
                    formatter,
                    "represented {field:?} {index} ({value}) does not round-trip exactly through f32"
                ),
                None => write!(
                    formatter,
                    "represented {field:?} ({value}) does not round-trip exactly through f32"
                ),
            },
            ScalarRefinementErrorKind::InvalidSplitValue {
                interval_index,
                part,
                value,
            } => write!(
                formatter,
                "split {part} value {value} for interval {interval_index} is not finite and positive"
            ),
            ScalarRefinementErrorKind::DurationConservationFailure { interval_index } => write!(
                formatter,
                "split durations for interval {interval_index} do not sum exactly to the represented duration"
            ),
            ScalarRefinementErrorKind::NonFiniteInsertedSample {
                interval_index,
                inserted_index,
                value,
            } => write!(
                formatter,
                "inserted sample {inserted_index} for interval {interval_index} is not finite after rounding ({value})"
            ),
            ScalarRefinementErrorKind::RecordValidation(source) => {
                write!(formatter, "refined record validation failed: {source}")
            }
        }
    }
}

impl std::error::Error for ScalarRefinementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ScalarRefinementErrorKind::RecordValidation(source) => Some(source),
            _ => None,
        }
    }
}

/// Construct the candidate-scalar one-third/two-thirds refinement of one
/// represented fixture case.
pub fn refine_represented_case(
    case: &RepresentedCase,
) -> Result<ScalarRefinement, ScalarRefinementError> {
    refine_record(
        case.record(),
        case.coordinate(),
        case.physical_cutoff(),
        case.candidate_cutoff_bits(),
        case.candidate_angular_cutoff_bits(),
    )
}

fn refine_record(
    source: &RepresentedRecord,
    coordinate: FixtureCoordinate,
    physical_cutoff: PhysicalCutoff,
    candidate_cutoff_bits: ScalarBits,
    candidate_angular_cutoff_bits: ScalarBits,
) -> Result<ScalarRefinement, ScalarRefinementError> {
    let scalar = source.scalar();
    let interval_count = source.intervals_seconds().len();
    let refined_interval_count = interval_count.checked_mul(3).ok_or_else(|| {
        error(
            coordinate,
            scalar,
            physical_cutoff,
            ScalarRefinementErrorKind::RefinedLengthOverflow { interval_count },
        )
    })?;
    let refined_sample_count = refined_interval_count.checked_add(1).ok_or_else(|| {
        error(
            coordinate,
            scalar,
            physical_cutoff,
            ScalarRefinementErrorKind::RefinedLengthOverflow { interval_count },
        )
    })?;

    let record = match scalar {
        RecordScalar::F32 => {
            let cutoff = narrow_f32(source.cutoff_hz(), RepresentedValueField::Cutoff, None)
                .map_err(|kind| error(coordinate, scalar, physical_cutoff, kind))?;
            let mut intervals = Vec::new();
            intervals
                .try_reserve_exact(refined_interval_count)
                .map_err(|_| {
                    error(
                        coordinate,
                        scalar,
                        physical_cutoff,
                        ScalarRefinementErrorKind::CapacityReservationFailed {
                            buffer: RefinedBuffer::Intervals,
                            requested_capacity: refined_interval_count,
                        },
                    )
                })?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(refined_sample_count)
                .map_err(|_| {
                    error(
                        coordinate,
                        scalar,
                        physical_cutoff,
                        ScalarRefinementErrorKind::CapacityReservationFailed {
                            buffer: RefinedBuffer::Samples,
                            requested_capacity: refined_sample_count,
                        },
                    )
                })?;
            refine_f32(
                source,
                coordinate,
                physical_cutoff,
                &mut intervals,
                &mut samples,
            )?;
            RepresentedRecord::from_f32(cutoff, intervals, samples)
        }
        RecordScalar::F64 => {
            let mut intervals = Vec::new();
            intervals
                .try_reserve_exact(refined_interval_count)
                .map_err(|_| {
                    error(
                        coordinate,
                        scalar,
                        physical_cutoff,
                        ScalarRefinementErrorKind::CapacityReservationFailed {
                            buffer: RefinedBuffer::Intervals,
                            requested_capacity: refined_interval_count,
                        },
                    )
                })?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(refined_sample_count)
                .map_err(|_| {
                    error(
                        coordinate,
                        scalar,
                        physical_cutoff,
                        ScalarRefinementErrorKind::CapacityReservationFailed {
                            buffer: RefinedBuffer::Samples,
                            requested_capacity: refined_sample_count,
                        },
                    )
                })?;
            refine_f64(
                source,
                coordinate,
                physical_cutoff,
                &mut intervals,
                &mut samples,
            )?;
            RepresentedRecord::from_f64(source.cutoff_hz(), intervals, samples)
        }
    }
    .map_err(|source| {
        error(
            coordinate,
            scalar,
            physical_cutoff,
            ScalarRefinementErrorKind::RecordValidation(source),
        )
    })?;

    Ok(ScalarRefinement {
        coordinate,
        physical_cutoff,
        candidate_cutoff_bits,
        candidate_angular_cutoff_bits,
        record,
        original_sample_count: source.samples().len(),
    })
}

fn refine_f32(
    source: &RepresentedRecord,
    coordinate: FixtureCoordinate,
    physical_cutoff: PhysicalCutoff,
    refined_intervals: &mut Vec<f32>,
    refined_samples: &mut Vec<f32>,
) -> Result<(), ScalarRefinementError> {
    let first_sample = narrow_f32(source.samples()[0], RepresentedValueField::Sample, Some(0))
        .map_err(|kind| error(coordinate, RecordScalar::F32, physical_cutoff, kind))?;
    refined_samples.push(first_sample);

    for interval_index in 0..source.intervals_seconds().len() {
        let duration = narrow_f32(
            source.intervals_seconds()[interval_index],
            RepresentedValueField::Interval,
            Some(interval_index),
        )
        .map_err(|kind| error(coordinate, RecordScalar::F32, physical_cutoff, kind))?;
        let split = split_f32(duration, interval_index)
            .map_err(|kind| error(coordinate, RecordScalar::F32, physical_cutoff, kind))?;
        refined_intervals.extend([
            split.third_duration,
            split.third_duration,
            split.remainder_duration,
        ]);

        let left = narrow_f32(
            source.samples()[interval_index],
            RepresentedValueField::Sample,
            Some(interval_index),
        )
        .map_err(|kind| error(coordinate, RecordScalar::F32, physical_cutoff, kind))?;
        let right = narrow_f32(
            source.samples()[interval_index + 1],
            RepresentedValueField::Sample,
            Some(interval_index + 1),
        )
        .map_err(|kind| error(coordinate, RecordScalar::F32, physical_cutoff, kind))?;

        for (inserted_index, numerator) in
            [(1, split.third_duration), (2, split.two_thirds_position)]
        {
            let interpolated = interpolate(
                f64::from(left),
                f64::from(right),
                f64::from(numerator),
                f64::from(duration),
            )
            .to_f32_round(Round::Nearest);
            if !interpolated.is_finite() {
                return Err(error(
                    coordinate,
                    RecordScalar::F32,
                    physical_cutoff,
                    ScalarRefinementErrorKind::NonFiniteInsertedSample {
                        interval_index,
                        inserted_index,
                        value: f64::from(interpolated),
                    },
                ));
            }
            refined_samples.push(interpolated);
        }

        // Preserve each original endpoint exactly as stored in the source.
        refined_samples.push(right);
    }

    Ok(())
}

fn refine_f64(
    source: &RepresentedRecord,
    coordinate: FixtureCoordinate,
    physical_cutoff: PhysicalCutoff,
    refined_intervals: &mut Vec<f64>,
    refined_samples: &mut Vec<f64>,
) -> Result<(), ScalarRefinementError> {
    refined_samples.push(source.samples()[0]);

    for interval_index in 0..source.intervals_seconds().len() {
        let duration = source.intervals_seconds()[interval_index];
        let split = split_f64(duration, interval_index)
            .map_err(|kind| error(coordinate, RecordScalar::F64, physical_cutoff, kind))?;
        refined_intervals.extend([
            split.third_duration,
            split.third_duration,
            split.remainder_duration,
        ]);

        let left = source.samples()[interval_index];
        let right = source.samples()[interval_index + 1];
        for (inserted_index, numerator) in
            [(1, split.third_duration), (2, split.two_thirds_position)]
        {
            let interpolated =
                interpolate(left, right, numerator, duration).to_f64_round(Round::Nearest);
            if !interpolated.is_finite() {
                return Err(error(
                    coordinate,
                    RecordScalar::F64,
                    physical_cutoff,
                    ScalarRefinementErrorKind::NonFiniteInsertedSample {
                        interval_index,
                        inserted_index,
                        value: interpolated,
                    },
                ));
            }
            refined_samples.push(interpolated);
        }

        // Preserve each original endpoint exactly as stored in the source.
        refined_samples.push(source.samples()[interval_index + 1]);
    }

    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct NativeSplit<T> {
    third_duration: T,
    two_thirds_position: T,
    remainder_duration: T,
}

fn split_f32(
    duration: f32,
    interval_index: usize,
) -> Result<NativeSplit<f32>, ScalarRefinementErrorKind> {
    let third_duration = duration / 3.0_f32;
    let two_thirds_position = 2.0_f32 * third_duration;
    let remainder_duration = duration - two_thirds_position;
    let split = NativeSplit {
        third_duration,
        two_thirds_position,
        remainder_duration,
    };

    for (part, value) in [
        (SplitPart::ThirdDuration, third_duration),
        (SplitPart::TwoThirdsPosition, two_thirds_position),
        (SplitPart::RemainderDuration, remainder_duration),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(ScalarRefinementErrorKind::InvalidSplitValue {
                interval_index,
                part,
                value: f64::from(value),
            });
        }
    }

    if !durations_conserve(
        f64::from(third_duration),
        f64::from(remainder_duration),
        f64::from(duration),
    ) {
        return Err(ScalarRefinementErrorKind::DurationConservationFailure { interval_index });
    }
    Ok(split)
}

fn split_f64(
    duration: f64,
    interval_index: usize,
) -> Result<NativeSplit<f64>, ScalarRefinementErrorKind> {
    let third_duration = duration / 3.0_f64;
    let two_thirds_position = 2.0_f64 * third_duration;
    let remainder_duration = duration - two_thirds_position;
    let split = NativeSplit {
        third_duration,
        two_thirds_position,
        remainder_duration,
    };

    for (part, value) in [
        (SplitPart::ThirdDuration, third_duration),
        (SplitPart::TwoThirdsPosition, two_thirds_position),
        (SplitPart::RemainderDuration, remainder_duration),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(ScalarRefinementErrorKind::InvalidSplitValue {
                interval_index,
                part,
                value,
            });
        }
    }

    if !durations_conserve(third_duration, remainder_duration, duration) {
        return Err(ScalarRefinementErrorKind::DurationConservationFailure { interval_index });
    }
    Ok(split)
}

fn durations_conserve(third_duration: f64, remainder_duration: f64, duration: f64) -> bool {
    // The 256-bit additions are exact for these promoted binary scalar values.
    let mut sum = Float::with_val(REFINEMENT_PRECISION, third_duration);
    sum += Float::with_val(REFINEMENT_PRECISION, third_duration);
    sum += Float::with_val(REFINEMENT_PRECISION, remainder_duration);
    sum == Float::with_val(REFINEMENT_PRECISION, duration)
}

fn interpolate(left: f64, right: f64, numerator: f64, duration: f64) -> Float {
    let mut theta = Float::with_val(REFINEMENT_PRECISION, numerator);
    theta /= Float::with_val(REFINEMENT_PRECISION, duration);

    let mut difference = Float::with_val(REFINEMENT_PRECISION, right);
    difference -= Float::with_val(REFINEMENT_PRECISION, left);

    let mut offset = theta;
    offset *= difference;

    let mut value = Float::with_val(REFINEMENT_PRECISION, left);
    value += offset;
    value
}

fn narrow_f32(
    value: f64,
    field: RepresentedValueField,
    index: Option<usize>,
) -> Result<f32, ScalarRefinementErrorKind> {
    let narrowed = value as f32;
    if !narrowed.is_finite() || f64::from(narrowed).to_bits() != value.to_bits() {
        return Err(ScalarRefinementErrorKind::NonExactF32Storage {
            field,
            index,
            value,
        });
    }
    Ok(narrowed)
}

fn error(
    coordinate: FixtureCoordinate,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
    kind: ScalarRefinementErrorKind,
) -> ScalarRefinementError {
    ScalarRefinementError {
        coordinate,
        scalar,
        cutoff,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use rug::{Float, float::Round};

    use super::*;
    use crate::{
        QMax, SCHEDULES, TIME_DOMAIN_FIXTURES, TimeDomainFixture, build_nominal_record,
        build_represented_case,
    };

    const TEST_PRECISION: u32 = 256;

    fn coordinate() -> FixtureCoordinate {
        FixtureCoordinate {
            fixture: TimeDomainFixture::UnequalMultitone,
            q_max: QMax::PiOver32,
            schedule: crate::ScheduleKind::Uniform,
        }
    }

    fn test_context(
        scalar: RecordScalar,
    ) -> (FixtureCoordinate, PhysicalCutoff, ScalarBits, ScalarBits) {
        let coordinate = coordinate();
        let cutoff = PhysicalCutoff::OneHz;
        match scalar {
            RecordScalar::F32 => (
                coordinate,
                cutoff,
                ScalarBits::F32(1.0_f32.to_bits()),
                ScalarBits::F32(core::f32::consts::TAU.to_bits()),
            ),
            RecordScalar::F64 => (
                coordinate,
                cutoff,
                ScalarBits::F64(1.0_f64.to_bits()),
                ScalarBits::F64(core::f64::consts::TAU.to_bits()),
            ),
        }
    }

    fn custom_refinement(
        scalar: RecordScalar,
        duration_f32: Option<f32>,
        duration_f64: Option<f64>,
        samples_f32: Option<Vec<f32>>,
        samples_f64: Option<Vec<f64>>,
    ) -> Result<ScalarRefinement, ScalarRefinementError> {
        let (coordinate, cutoff, cutoff_bits, omega_bits) = test_context(scalar);
        let record = match scalar {
            RecordScalar::F32 => RepresentedRecord::from_f32(
                1.0,
                vec![duration_f32.expect("f32 duration")],
                samples_f32.expect("f32 samples"),
            )
            .unwrap(),
            RecordScalar::F64 => RepresentedRecord::from_f64(
                1.0,
                vec![duration_f64.expect("f64 duration")],
                samples_f64.expect("f64 samples"),
            )
            .unwrap(),
        };
        refine_record(&record, coordinate, cutoff, cutoff_bits, omega_bits)
    }

    fn exact_high_precision_interpolation(
        left: f64,
        right: f64,
        numerator: f64,
        duration: f64,
    ) -> Float {
        let mut difference = Float::with_val(TEST_PRECISION, right);
        difference -= Float::with_val(TEST_PRECISION, left);
        let mut fraction = Float::with_val(TEST_PRECISION, numerator);
        fraction /= Float::with_val(TEST_PRECISION, duration);
        let mut weighted_difference = difference;
        weighted_difference *= fraction;
        let mut interpolated = Float::with_val(TEST_PRECISION, left);
        interpolated += weighted_difference;
        interpolated
    }

    fn sample_bits(record: &RepresentedRecord) -> Vec<u64> {
        record
            .samples()
            .iter()
            .map(|value| value.to_bits())
            .collect()
    }

    fn interval_bits(record: &RepresentedRecord) -> Vec<u64> {
        record
            .intervals_seconds()
            .iter()
            .map(|value| value.to_bits())
            .collect()
    }

    #[test]
    fn native_scalar_split_bits_are_frozen() {
        let duration_f32 = f32::from_bits(0x3dcccccd);
        let split32 = split_f32(duration_f32, 0).unwrap();
        assert_eq!(split32.third_duration.to_bits(), 0x3d088889);
        assert_eq!(split32.two_thirds_position.to_bits(), 0x3d888889);
        assert_eq!(split32.remainder_duration.to_bits(), 0x3d088888);

        let duration_f64 = f64::from_bits(0x3fb9_9999_9999_999a);
        let split64 = split_f64(duration_f64, 0).unwrap();
        assert_eq!(split64.third_duration.to_bits(), 0x3fa1_1111_1111_1111);
        assert_eq!(split64.two_thirds_position.to_bits(), 0x3fb1_1111_1111_1111);
        assert_eq!(split64.remainder_duration.to_bits(), 0x3fa1_1111_1111_1112);
    }

    #[test]
    fn f32_sentinel_uses_native_split_and_256_bit_round_once_samples() {
        let duration = f32::from_bits(0x394f_ead7);
        let left = f32::from_bits(0xbccc_0c0c);
        let right = f32::from_bits(0x3d4c_0c04);
        let split = split_f32(duration, 0).unwrap();
        assert_eq!(split.third_duration.to_bits(), 0x388a_9c8f);
        assert_eq!(split.two_thirds_position.to_bits(), 0x390a_9c8f);
        assert_eq!(split.remainder_duration.to_bits(), 0x388a_9c90);
        let widened_unrounded_first = f64::from(duration) / 3.0_f64;
        assert_ne!(
            f64::from(split.third_duration).to_bits(),
            widened_unrounded_first.to_bits()
        );

        let refined = custom_refinement(
            RecordScalar::F32,
            Some(duration),
            None,
            Some(vec![left, right]),
            None,
        )
        .unwrap();
        let inserted_first = refined.record().samples()[1] as f32;
        let inserted_second = refined.record().samples()[2] as f32;
        assert_eq!(inserted_first.to_bits(), 0xb23a_5e6b);
        assert_eq!(inserted_second.to_bits(), 0x3ccc_0c00);

        let expected_first = exact_high_precision_interpolation(
            f64::from(left),
            f64::from(right),
            f64::from(split.third_duration),
            f64::from(duration),
        )
        .to_f32_round(Round::Nearest);
        let expected_second = exact_high_precision_interpolation(
            f64::from(left),
            f64::from(right),
            f64::from(split.two_thirds_position),
            f64::from(duration),
        )
        .to_f32_round(Round::Nearest);
        assert_eq!(inserted_first.to_bits(), expected_first.to_bits());
        assert_eq!(inserted_second.to_bits(), expected_second.to_bits());

        let first_sequential_f64 = (f64::from(left)
            + (f64::from(split.third_duration) / f64::from(duration))
                * (f64::from(right) - f64::from(left))) as f32;
        let first_staged_f32 = left + (split.third_duration / duration) * (right - left);
        assert_eq!(first_sequential_f64.to_bits(), 0xb23a_5e6c);
        assert_eq!(first_staged_f32.to_bits(), 0xb240_0000);
        assert_ne!(inserted_first.to_bits(), first_sequential_f64.to_bits());
        assert_ne!(inserted_first.to_bits(), first_staged_f32.to_bits());
    }

    #[test]
    fn f64_inserted_samples_match_independent_256_bit_interpolation() {
        let duration = f64::from_bits(0x3fb9_9999_9999_999a);
        let left = f64::from_bits(0xbfd9_9999_9999_999a);
        let right = f64::from_bits(0x3fe6_6666_6666_6666);
        let split = split_f64(duration, 0).unwrap();
        let refined = custom_refinement(
            RecordScalar::F64,
            None,
            Some(duration),
            None,
            Some(vec![left, right]),
        )
        .unwrap();

        let expected_first =
            exact_high_precision_interpolation(left, right, split.third_duration, duration)
                .to_f64_round(Round::Nearest);
        let expected_second =
            exact_high_precision_interpolation(left, right, split.two_thirds_position, duration)
                .to_f64_round(Round::Nearest);
        assert_eq!(
            refined.record().samples()[1].to_bits(),
            expected_first.to_bits()
        );
        assert_eq!(
            refined.record().samples()[2].to_bits(),
            expected_second.to_bits()
        );
        assert_eq!(refined.record().samples()[0].to_bits(), left.to_bits());
        assert_eq!(refined.record().samples()[3].to_bits(), right.to_bits());
    }

    #[test]
    fn min_subnormal_splits_reject_zero_parts_and_accept_three_ulp_duration() {
        for scalar in [RecordScalar::F32, RecordScalar::F64] {
            let (one, two, three) = match scalar {
                RecordScalar::F32 => (
                    custom_refinement(
                        scalar,
                        Some(f32::from_bits(1)),
                        None,
                        Some(vec![0.0, 1.0]),
                        None,
                    ),
                    custom_refinement(
                        scalar,
                        Some(f32::from_bits(2)),
                        None,
                        Some(vec![0.0, 1.0]),
                        None,
                    ),
                    custom_refinement(
                        scalar,
                        Some(f32::from_bits(3)),
                        None,
                        Some(vec![0.0, 1.0]),
                        None,
                    ),
                ),
                RecordScalar::F64 => (
                    custom_refinement(
                        scalar,
                        None,
                        Some(f64::from_bits(1)),
                        None,
                        Some(vec![0.0, 1.0]),
                    ),
                    custom_refinement(
                        scalar,
                        None,
                        Some(f64::from_bits(2)),
                        None,
                        Some(vec![0.0, 1.0]),
                    ),
                    custom_refinement(
                        scalar,
                        None,
                        Some(f64::from_bits(3)),
                        None,
                        Some(vec![0.0, 1.0]),
                    ),
                ),
            };

            assert!(matches!(
                one.unwrap_err().kind(),
                ScalarRefinementErrorKind::InvalidSplitValue {
                    interval_index: 0,
                    part: SplitPart::ThirdDuration,
                    value: 0.0,
                }
            ));
            assert!(matches!(
                two.unwrap_err().kind(),
                ScalarRefinementErrorKind::InvalidSplitValue {
                    interval_index: 0,
                    part: SplitPart::RemainderDuration,
                    value: 0.0,
                }
            ));

            let accepted = three.unwrap();
            assert_eq!(accepted.record().intervals_seconds().len(), 3);
            match scalar {
                RecordScalar::F32 => assert_eq!(
                    accepted
                        .record()
                        .intervals_seconds()
                        .iter()
                        .map(|value| (*value as f32).to_bits())
                        .collect::<Vec<_>>(),
                    [1, 1, 1]
                ),
                RecordScalar::F64 => assert_eq!(
                    accepted
                        .record()
                        .intervals_seconds()
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    [1, 1, 1]
                ),
            }
        }
    }

    #[test]
    fn error_display_and_record_source_keep_context() {
        let coordinate = coordinate();
        let record_error = RecordError::InvalidCutoff;
        let contextual = error(
            coordinate,
            RecordScalar::F64,
            PhysicalCutoff::ThirtySevenHz,
            ScalarRefinementErrorKind::RecordValidation(record_error.clone()),
        );
        assert!(contextual.to_string().contains("record validation failed"));
        assert_eq!(
            contextual.source().unwrap().to_string(),
            record_error.to_string()
        );

        let split = custom_refinement(
            RecordScalar::F32,
            Some(f32::from_bits(1)),
            None,
            Some(vec![0.0, 1.0]),
            None,
        )
        .unwrap_err();
        assert!(split.to_string().contains("split third duration"));
        assert!(split.to_string().contains("interval 0"));
        assert!(split.source().is_none());
    }

    #[test]
    fn original_index_mapping_checks_bounds_and_overflow() {
        let refinement = custom_refinement(
            RecordScalar::F64,
            None,
            Some(0.75),
            None,
            Some(vec![-0.25, 0.5]),
        )
        .unwrap();
        assert_eq!(refinement.refined_index_for_original(0), Some(0));
        assert_eq!(refinement.refined_index_for_original(1), Some(3));
        assert_eq!(refinement.refined_index_for_original(2), None);
        assert_eq!(refinement.refined_index_for_original(usize::MAX), None);
    }

    #[test]
    fn refinement_is_deterministic_and_copies_source_metadata() {
        let coordinate = coordinate();
        let nominal = build_nominal_record(coordinate).unwrap();
        let case =
            build_represented_case(&nominal, RecordScalar::F32, PhysicalCutoff::ThirtySevenHz)
                .unwrap();
        let first = refine_represented_case(&case).unwrap();
        let second = refine_represented_case(&case).unwrap();

        assert_eq!(first.coordinate(), case.coordinate());
        assert_eq!(first.physical_cutoff(), case.physical_cutoff());
        assert_eq!(first.candidate_cutoff_bits(), case.candidate_cutoff_bits());
        assert_eq!(
            first.candidate_angular_cutoff_bits(),
            case.candidate_angular_cutoff_bits()
        );
        assert_eq!(first.original_sample_count(), case.record().samples().len());
        assert_eq!(sample_bits(first.record()), sample_bits(second.record()));
        assert_eq!(
            interval_bits(first.record()),
            interval_bits(second.record())
        );
    }

    #[test]
    fn every_frozen_case_refines_one_record_at_a_time() {
        let mut case_count = 0;
        let mut largest_base = (0, 0);
        let mut largest_refined = (0, 0);
        let mut largest_coordinate = None;

        for fixture in TIME_DOMAIN_FIXTURES {
            for q_max in crate::Q_MAXES {
                for schedule in SCHEDULES {
                    let coordinate = FixtureCoordinate {
                        fixture,
                        q_max,
                        schedule,
                    };
                    let nominal = build_nominal_record(coordinate).unwrap();
                    for scalar in [RecordScalar::F32, RecordScalar::F64] {
                        for cutoff in [PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz] {
                            let case = build_represented_case(&nominal, scalar, cutoff).unwrap();
                            let refined = refine_represented_case(&case).unwrap();
                            let base_interval_count = case.record().intervals_seconds().len();
                            let refined_interval_count =
                                base_interval_count.checked_mul(3).unwrap();
                            let refined_sample_count =
                                refined_interval_count.checked_add(1).unwrap();

                            assert_eq!(refined.coordinate(), coordinate);
                            assert_eq!(refined.physical_cutoff(), cutoff);
                            assert_eq!(
                                refined.candidate_cutoff_bits(),
                                case.candidate_cutoff_bits()
                            );
                            assert_eq!(
                                refined.candidate_angular_cutoff_bits(),
                                case.candidate_angular_cutoff_bits()
                            );
                            assert_eq!(
                                refined.original_sample_count(),
                                case.record().samples().len()
                            );
                            assert_eq!(
                                refined.record().intervals_seconds().len(),
                                refined_interval_count
                            );
                            assert_eq!(refined.record().samples().len(), refined_sample_count);
                            assert_eq!(
                                refined.record().scale(TEST_PRECISION),
                                case.record().scale(TEST_PRECISION)
                            );

                            for (original_index, original_sample) in
                                case.record().samples().iter().copied().enumerate()
                            {
                                let refined_index =
                                    refined.refined_index_for_original(original_index).unwrap();
                                assert_eq!(refined_index, original_index.checked_mul(3).unwrap());
                                assert_eq!(
                                    refined.record().samples()[refined_index].to_bits(),
                                    original_sample.to_bits()
                                );
                            }
                            assert_eq!(
                                refined.refined_index_for_original(case.record().samples().len()),
                                None
                            );

                            for interval_index in 0..base_interval_count {
                                let base_duration = Float::with_val(
                                    TEST_PRECISION,
                                    case.record().intervals_seconds()[interval_index],
                                );
                                let refined_start = interval_index * 3;
                                let parts = &refined.record().intervals_seconds()
                                    [refined_start..refined_start + 3];
                                assert!(parts.iter().all(|part| part.is_finite() && *part > 0.0));

                                match scalar {
                                    RecordScalar::F32 => {
                                        let first = parts[0] as f32;
                                        let second = parts[1] as f32;
                                        assert_eq!(second.to_bits(), first.to_bits());
                                    }
                                    RecordScalar::F64 => {
                                        assert_eq!(parts[1].to_bits(), parts[0].to_bits());
                                    }
                                }

                                let mut sum = Float::with_val(TEST_PRECISION, parts[0]);
                                sum += Float::with_val(TEST_PRECISION, parts[0]);
                                sum += Float::with_val(TEST_PRECISION, parts[2]);
                                assert_eq!(
                                    sum, base_duration,
                                    "{coordinate:?} {scalar:?} {interval_index}"
                                );
                            }

                            if base_interval_count > largest_base.0 {
                                largest_base = (base_interval_count, case.record().samples().len());
                                largest_coordinate = Some(coordinate);
                            }
                            largest_refined.0 = largest_refined
                                .0
                                .max(refined.record().intervals_seconds().len());
                            largest_refined.1 =
                                largest_refined.1.max(refined.record().samples().len());
                            case_count += 1;
                            drop(refined);
                            drop(case);
                        }
                    }
                }
            }
        }

        assert_eq!(case_count, 7 * 3 * 3 * 2 * 2);
        assert_eq!(largest_base, (39_819, 39_820));
        assert_eq!(largest_refined, (119_457, 119_458));
        assert_eq!(
            largest_coordinate,
            Some(FixtureCoordinate {
                fixture: TimeDomainFixture::UnequalMultitone,
                q_max: QMax::PiOver128,
                schedule: crate::ScheduleKind::Uneven,
            })
        );
    }
}
