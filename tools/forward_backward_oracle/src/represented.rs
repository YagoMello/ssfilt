//! Exact candidate-scalar conversion for frozen nominal fixture records.
//!
//! Each interval is materialized in this order at 256-bit precision:
//! `q_i = pi * tick_delta / tick_denominator`, followed by
//! `dt_i = q_i / (2 * pi * promote(fc_T))`.  The `pi` factors are deliberately
//! retained: cancelling them would change the prescribed rounding path.

use core::fmt;

use rug::{
    Float,
    float::{Constant, Round},
};

use crate::{FixtureCoordinate, NominalRecord, RecordError, RecordScalar, RepresentedRecord};

const CONVERSION_PRECISION: u32 = 256;

/// A frozen physical cutoff used by the forward-backward candidate matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalCutoff {
    OneHz,
    ThirtySevenHz,
}

/// Stable declaration order for the physical-cutoff matrix axis.
pub const PHYSICAL_CUTOFFS: [PhysicalCutoff; 2] =
    [PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz];

impl PhysicalCutoff {
    /// The exact physical frequency represented by this frozen coordinate.
    #[must_use]
    pub const fn hz(self) -> f64 {
        match self {
            Self::OneHz => 1.0,
            Self::ThirtySevenHz => 37.0,
        }
    }
}

/// Stable scalar declaration order. The scalar semantics remain owned by
/// [`RecordScalar`].
pub const SCALAR_ORDER: [RecordScalar; 2] = [RecordScalar::F32, RecordScalar::F64];

/// Exact native-width bits for a candidate scalar value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarBits {
    F32(u32),
    F64(u64),
}

impl ScalarBits {
    #[must_use]
    pub const fn scalar(self) -> RecordScalar {
        match self {
            Self::F32(_) => RecordScalar::F32,
            Self::F64(_) => RecordScalar::F64,
        }
    }
}

/// The complete candidate representation of one nominal fixture record.
#[derive(Clone, Debug)]
pub struct RepresentedCase {
    coordinate: FixtureCoordinate,
    physical_cutoff: PhysicalCutoff,
    record: RepresentedRecord,
    candidate_cutoff_bits: ScalarBits,
    candidate_angular_cutoff_bits: ScalarBits,
}

impl RepresentedCase {
    #[must_use]
    pub const fn coordinate(&self) -> FixtureCoordinate {
        self.coordinate
    }

    #[must_use]
    pub const fn physical_cutoff(&self) -> PhysicalCutoff {
        self.physical_cutoff
    }

    #[must_use]
    pub const fn record(&self) -> &RepresentedRecord {
        &self.record
    }

    /// Exact scalar bits of `fc_T` passed to the candidate.
    #[must_use]
    pub const fn candidate_cutoff_bits(&self) -> ScalarBits {
        self.candidate_cutoff_bits
    }

    /// Exact scalar bits of the candidate's native `TAU_T * fc_T` operation.
    #[must_use]
    pub const fn candidate_angular_cutoff_bits(&self) -> ScalarBits {
        self.candidate_angular_cutoff_bits
    }
}

/// A contextual failure while converting one nominal record for a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepresentedBuildError {
    pub coordinate: FixtureCoordinate,
    pub scalar: RecordScalar,
    pub cutoff: PhysicalCutoff,
    pub kind: RepresentedBuildErrorKind,
}

/// The conversion failure, with interval indices kept separate from context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepresentedBuildErrorKind {
    ShapeMismatch {
        tick_count: usize,
        timestamp_count: usize,
        sample_count: usize,
    },
    NonIncreasingTickPosition {
        index: usize,
        previous: u64,
        current: u64,
    },
    ZeroTickDelta {
        interval_index: usize,
    },
    TickDeltaOverflow {
        interval_index: usize,
    },
    InvalidRoundedCutoff,
    InvalidAngularCutoff,
    NonFiniteDuration {
        interval_index: usize,
    },
    NonPositiveDuration {
        interval_index: usize,
    },
    NonFiniteRoundedSample {
        sample_index: usize,
    },
    Record(RecordError),
}

impl fmt::Display for RepresentedBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "represented conversion failed for {:?}, {:?}, {:?}: ",
            self.coordinate, self.scalar, self.cutoff
        )?;
        match &self.kind {
            RepresentedBuildErrorKind::ShapeMismatch {
                tick_count,
                timestamp_count,
                sample_count,
            } => write!(
                formatter,
                "record shape has {tick_count} ticks, {timestamp_count} timestamps, and {sample_count} samples"
            ),
            RepresentedBuildErrorKind::NonIncreasingTickPosition {
                index,
                previous,
                current,
            } => write!(
                formatter,
                "tick position {index} ({current}) does not exceed its predecessor ({previous})"
            ),
            RepresentedBuildErrorKind::ZeroTickDelta { interval_index } => {
                write!(formatter, "tick delta at interval {interval_index} is zero")
            }
            RepresentedBuildErrorKind::TickDeltaOverflow { interval_index } => {
                write!(
                    formatter,
                    "tick delta at interval {interval_index} overflowed"
                )
            }
            RepresentedBuildErrorKind::InvalidRoundedCutoff => {
                formatter.write_str("rounded candidate cutoff is not finite and positive")
            }
            RepresentedBuildErrorKind::InvalidAngularCutoff => {
                formatter.write_str("candidate angular cutoff is not finite and positive")
            }
            RepresentedBuildErrorKind::NonFiniteDuration { interval_index } => write!(
                formatter,
                "rounded duration at interval {interval_index} is not finite"
            ),
            RepresentedBuildErrorKind::NonPositiveDuration { interval_index } => write!(
                formatter,
                "rounded duration at interval {interval_index} is not positive"
            ),
            RepresentedBuildErrorKind::NonFiniteRoundedSample { sample_index } => write!(
                formatter,
                "rounded sample at index {sample_index} is not finite"
            ),
            RepresentedBuildErrorKind::Record(source) => {
                write!(formatter, "represented-record validation failed: {source}")
            }
        }
    }
}

impl std::error::Error for RepresentedBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            RepresentedBuildErrorKind::Record(source) => Some(source),
            _ => None,
        }
    }
}

/// Convert exactly one shared-duration nominal record to one frozen candidate
/// scalar and cutoff.
pub fn build_represented_case(
    nominal: &NominalRecord,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
) -> Result<RepresentedCase, RepresentedBuildError> {
    let coordinate = nominal.coordinate();
    validate_nominal_shape(nominal, scalar, cutoff)?;

    match scalar {
        RecordScalar::F32 => build_f32(nominal, coordinate, cutoff),
        RecordScalar::F64 => build_f64(nominal, coordinate, cutoff),
    }
}

fn validate_nominal_shape(
    nominal: &NominalRecord,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
) -> Result<(), RepresentedBuildError> {
    let tick_count = nominal.tick_positions().len();
    let timestamp_count = nominal.normalized_times().len();
    let sample_count = nominal.samples().len();
    if tick_count < 2 || tick_count != timestamp_count || tick_count != sample_count {
        return Err(error(
            nominal.coordinate(),
            scalar,
            cutoff,
            RepresentedBuildErrorKind::ShapeMismatch {
                tick_count,
                timestamp_count,
                sample_count,
            },
        ));
    }

    for index in 1..tick_count {
        let previous = nominal.tick_positions()[index - 1];
        let current = nominal.tick_positions()[index];
        if current == previous {
            return Err(error(
                nominal.coordinate(),
                scalar,
                cutoff,
                RepresentedBuildErrorKind::ZeroTickDelta {
                    interval_index: index - 1,
                },
            ));
        }
        if current < previous {
            return Err(error(
                nominal.coordinate(),
                scalar,
                cutoff,
                RepresentedBuildErrorKind::NonIncreasingTickPosition {
                    index,
                    previous,
                    current,
                },
            ));
        }
    }
    Ok(())
}

fn build_f32(
    nominal: &NominalRecord,
    coordinate: FixtureCoordinate,
    cutoff: PhysicalCutoff,
) -> Result<RepresentedCase, RepresentedBuildError> {
    let cutoff_hz = cutoff.hz() as f32;
    if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
        return Err(error(
            coordinate,
            RecordScalar::F32,
            cutoff,
            RepresentedBuildErrorKind::InvalidRoundedCutoff,
        ));
    }
    let angular_cutoff = core::f32::consts::TAU * cutoff_hz;
    if !angular_cutoff.is_finite() || angular_cutoff <= 0.0 {
        return Err(error(
            coordinate,
            RecordScalar::F32,
            cutoff,
            RepresentedBuildErrorKind::InvalidAngularCutoff,
        ));
    }

    let denominator = coordinate.q_max.tick_denominator();
    let mut intervals_seconds = Vec::with_capacity(nominal.tick_positions().len() - 1);
    for (interval_index, ticks) in nominal.tick_positions().windows(2).enumerate() {
        let delta = ticks[1].checked_sub(ticks[0]).ok_or_else(|| {
            error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::TickDeltaOverflow { interval_index },
            )
        })?;
        if delta == 0 {
            return Err(error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::ZeroTickDelta { interval_index },
            ));
        }
        let nominal_interval = nominal_interval(delta, denominator);
        let quotient = physical_duration_quotient(&nominal_interval, f64::from(cutoff_hz));
        let duration = quotient.to_f32_round(Round::Down);
        if !duration.is_finite() {
            return Err(error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteDuration { interval_index },
            ));
        }
        if duration <= 0.0 {
            return Err(error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::NonPositiveDuration { interval_index },
            ));
        }
        intervals_seconds.push(duration);
    }

    let mut samples = Vec::with_capacity(nominal.samples().len());
    for (sample_index, sample) in nominal.samples().iter().copied().enumerate() {
        let rounded = sample as f32;
        if !rounded.is_finite() {
            return Err(error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteRoundedSample { sample_index },
            ));
        }
        samples.push(rounded);
    }

    let record =
        RepresentedRecord::from_f32(cutoff_hz, intervals_seconds, samples).map_err(|source| {
            error(
                coordinate,
                RecordScalar::F32,
                cutoff,
                RepresentedBuildErrorKind::Record(source),
            )
        })?;
    Ok(RepresentedCase {
        coordinate,
        physical_cutoff: cutoff,
        record,
        candidate_cutoff_bits: ScalarBits::F32(cutoff_hz.to_bits()),
        candidate_angular_cutoff_bits: ScalarBits::F32(angular_cutoff.to_bits()),
    })
}

fn build_f64(
    nominal: &NominalRecord,
    coordinate: FixtureCoordinate,
    cutoff: PhysicalCutoff,
) -> Result<RepresentedCase, RepresentedBuildError> {
    let cutoff_hz = cutoff.hz();
    if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
        return Err(error(
            coordinate,
            RecordScalar::F64,
            cutoff,
            RepresentedBuildErrorKind::InvalidRoundedCutoff,
        ));
    }
    let angular_cutoff = core::f64::consts::TAU * cutoff_hz;
    if !angular_cutoff.is_finite() || angular_cutoff <= 0.0 {
        return Err(error(
            coordinate,
            RecordScalar::F64,
            cutoff,
            RepresentedBuildErrorKind::InvalidAngularCutoff,
        ));
    }

    let denominator = coordinate.q_max.tick_denominator();
    let mut intervals_seconds = Vec::with_capacity(nominal.tick_positions().len() - 1);
    for (interval_index, ticks) in nominal.tick_positions().windows(2).enumerate() {
        let delta = ticks[1].checked_sub(ticks[0]).ok_or_else(|| {
            error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::TickDeltaOverflow { interval_index },
            )
        })?;
        if delta == 0 {
            return Err(error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::ZeroTickDelta { interval_index },
            ));
        }
        let nominal_interval = nominal_interval(delta, denominator);
        let quotient = physical_duration_quotient(&nominal_interval, cutoff_hz);
        let duration = quotient.to_f64_round(Round::Down);
        if !duration.is_finite() {
            return Err(error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteDuration { interval_index },
            ));
        }
        if duration <= 0.0 {
            return Err(error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::NonPositiveDuration { interval_index },
            ));
        }
        intervals_seconds.push(duration);
    }

    let mut samples = Vec::with_capacity(nominal.samples().len());
    for (sample_index, sample) in nominal.samples().iter().copied().enumerate() {
        if !sample.is_finite() {
            return Err(error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteRoundedSample { sample_index },
            ));
        }
        samples.push(sample);
    }

    let record =
        RepresentedRecord::from_f64(cutoff_hz, intervals_seconds, samples).map_err(|source| {
            error(
                coordinate,
                RecordScalar::F64,
                cutoff,
                RepresentedBuildErrorKind::Record(source),
            )
        })?;
    Ok(RepresentedCase {
        coordinate,
        physical_cutoff: cutoff,
        record,
        candidate_cutoff_bits: ScalarBits::F64(cutoff_hz.to_bits()),
        candidate_angular_cutoff_bits: ScalarBits::F64(angular_cutoff.to_bits()),
    })
}

fn nominal_interval(delta: u64, tick_denominator: u64) -> Float {
    let mut nominal = Float::with_val(CONVERSION_PRECISION, Constant::Pi);
    nominal *= delta;
    nominal /= tick_denominator;
    nominal
}

fn physical_duration_quotient(nominal_interval: &Float, cutoff_hz: f64) -> Float {
    let mut denominator = Float::with_val(CONVERSION_PRECISION, Constant::Pi);
    denominator *= 2;
    let promoted_cutoff = Float::with_val(CONVERSION_PRECISION, cutoff_hz);
    denominator *= promoted_cutoff;

    let mut quotient = nominal_interval.clone();
    quotient /= denominator;
    quotient
}

fn error(
    coordinate: FixtureCoordinate,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
    kind: RepresentedBuildErrorKind,
) -> RepresentedBuildError {
    RepresentedBuildError {
        coordinate,
        scalar,
        cutoff,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use rug::{Float, float::Constant};

    use super::*;
    use crate::{QMax, SCHEDULES, TIME_DOMAIN_FIXTURES, TimeDomainFixture, build_nominal_record};

    const REFERENCE_PRECISION: u32 = 256;

    fn coordinate(
        fixture: TimeDomainFixture,
        q_max: QMax,
        schedule: crate::ScheduleKind,
    ) -> FixtureCoordinate {
        FixtureCoordinate {
            fixture,
            q_max,
            schedule,
        }
    }

    fn reference_nominal_interval(delta: u64, denominator: u64) -> Float {
        let mut pi_delta = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        let integer_delta = Float::with_val(REFERENCE_PRECISION, delta);
        pi_delta *= integer_delta;
        let integer_denominator = Float::with_val(REFERENCE_PRECISION, denominator);
        pi_delta /= integer_denominator;
        pi_delta
    }

    fn reference_physical_duration(q_i: &Float, cutoff_hz: f64) -> Float {
        let pi = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        let two = Float::with_val(REFERENCE_PRECISION, 2);
        let mut denominator = Float::with_val(REFERENCE_PRECISION, pi * two);
        let cutoff = Float::with_val(REFERENCE_PRECISION, cutoff_hz);
        denominator *= cutoff;
        let mut quotient = Float::with_val(REFERENCE_PRECISION, q_i);
        quotient /= denominator;
        quotient
    }

    fn next_up_f32(value: f32) -> f32 {
        assert!(value.is_finite() && value > 0.0);
        f32::from_bits(value.to_bits() + 1)
    }

    fn next_up_f64(value: f64) -> f64 {
        assert!(value.is_finite() && value > 0.0);
        f64::from_bits(value.to_bits() + 1)
    }

    fn duration_bits(case: &RepresentedCase, index: usize) -> u64 {
        match case.record.scalar() {
            RecordScalar::F32 => (case.record.intervals_seconds()[index] as f32).to_bits() as u64,
            RecordScalar::F64 => case.record.intervals_seconds()[index].to_bits(),
        }
    }

    fn check_full_precision_geometry(case: &RepresentedCase, nominal: &NominalRecord) {
        assert_eq!(case.coordinate(), nominal.coordinate());
        assert_eq!(
            case.record().scalar(),
            case.candidate_cutoff_bits().scalar()
        );
        assert_eq!(case.record().samples().len(), nominal.samples().len());
        assert_eq!(
            case.record().intervals_seconds().len() + 1,
            case.record().samples().len()
        );
        assert!(
            case.record()
                .samples()
                .iter()
                .all(|sample| sample.is_finite())
        );

        let scalar = case.record().scalar();
        let fc = case.record().cutoff_hz();
        let mut q_max = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        q_max /= nominal.coordinate().q_max.denominator();

        for (index, ticks) in nominal.tick_positions().windows(2).enumerate() {
            let delta = ticks[1] - ticks[0];
            assert!(delta > 0);
            let q_i =
                reference_nominal_interval(delta, nominal.coordinate().q_max.tick_denominator());
            let dt = case.record().intervals_seconds()[index];
            assert!(dt.is_finite() && dt > 0.0, "duration {index}");

            let mut q_hat = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
            q_hat *= 2;
            q_hat *= Float::with_val(REFERENCE_PRECISION, fc);
            q_hat *= Float::with_val(REFERENCE_PRECISION, dt);
            assert!(q_hat <= q_i, "represented q exceeds nominal q at {index}");
            assert!(q_i <= q_max, "nominal q exceeds q_max at {index}");

            let expected_duration = reference_physical_duration(&q_i, fc);
            match scalar {
                RecordScalar::F32 => {
                    let represented = dt as f32;
                    let next = next_up_f32(represented);
                    assert!(represented > 0.0 && represented.is_finite());
                    assert!(Float::with_val(REFERENCE_PRECISION, represented) <= expected_duration);
                    assert!(Float::with_val(REFERENCE_PRECISION, next) > expected_duration);
                }
                RecordScalar::F64 => {
                    let next = next_up_f64(dt);
                    assert!(Float::with_val(REFERENCE_PRECISION, dt) <= expected_duration);
                    assert!(Float::with_val(REFERENCE_PRECISION, next) > expected_duration);
                }
            }

            let expected_sample_bits = match scalar {
                RecordScalar::F32 => (nominal.samples()[index] as f32).to_bits() as u64,
                RecordScalar::F64 => nominal.samples()[index].to_bits(),
            };
            assert_eq!(
                sample_bits(case, index),
                expected_sample_bits,
                "sample {index}"
            );
        }

        let last = nominal.samples().len() - 1;
        let expected_last_bits = match scalar {
            RecordScalar::F32 => (nominal.samples()[last] as f32).to_bits() as u64,
            RecordScalar::F64 => nominal.samples()[last].to_bits(),
        };
        assert_eq!(sample_bits(case, last), expected_last_bits);

        let floor = match scalar {
            RecordScalar::F32 => 1.0e-2_f64,
            RecordScalar::F64 => 1.0e-3_f64,
        };
        let expected_scale = nominal
            .samples()
            .iter()
            .copied()
            .map(|sample| match scalar {
                RecordScalar::F32 => f64::from(sample as f32).abs(),
                RecordScalar::F64 => sample.abs(),
            })
            .fold(floor, f64::max);
        assert_eq!(
            case.record().scale(REFERENCE_PRECISION),
            Float::with_val(REFERENCE_PRECISION, expected_scale)
        );
    }

    fn sample_bits(case: &RepresentedCase, index: usize) -> u64 {
        match case.record.scalar() {
            RecordScalar::F32 => (case.record.samples()[index] as f32).to_bits() as u64,
            RecordScalar::F64 => case.record.samples()[index].to_bits(),
        }
    }

    #[test]
    fn frozen_axis_orders_are_explicit() {
        assert_eq!(
            PHYSICAL_CUTOFFS,
            [PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz]
        );
        assert_eq!(SCALAR_ORDER, [RecordScalar::F32, RecordScalar::F64]);
    }

    #[test]
    fn native_cutoff_and_angular_cutoff_bits_are_frozen() {
        let nominal = build_nominal_record(coordinate(
            TimeDomainFixture::UnequalMultitone,
            QMax::PiOver32,
            crate::ScheduleKind::Uneven,
        ))
        .unwrap();

        for cutoff in PHYSICAL_CUTOFFS {
            let hz = cutoff.hz();
            let f32_cutoff = hz as f32;
            let f32_omega = core::f32::consts::TAU * f32_cutoff;
            let f64_omega = core::f64::consts::TAU * hz;
            let f32_case = build_represented_case(&nominal, RecordScalar::F32, cutoff).unwrap();
            let f64_case = build_represented_case(&nominal, RecordScalar::F64, cutoff).unwrap();

            assert_eq!(
                f32_case.candidate_cutoff_bits(),
                ScalarBits::F32(f32_cutoff.to_bits())
            );
            assert_eq!(
                f64_case.candidate_cutoff_bits(),
                ScalarBits::F64(hz.to_bits())
            );
            assert_eq!(
                f32_case.candidate_angular_cutoff_bits(),
                ScalarBits::F32(f32_omega.to_bits())
            );
            assert_eq!(
                f64_case.candidate_angular_cutoff_bits(),
                ScalarBits::F64(f64_omega.to_bits())
            );
            let (
                expected_cutoff_f32,
                expected_omega_f32,
                expected_cutoff_f64,
                expected_omega_f64,
                expected_durations_f32,
                expected_durations_f64,
            ): (u32, u32, u64, u64, [u32; 2], [u64; 2]) = match cutoff {
                PhysicalCutoff::OneHz => (
                    0x3f80_0000,
                    0x40c9_0fdb,
                    0x3ff0_0000_0000_0000,
                    0x4019_21fb_5444_2d18,
                    [0x3c80_0000, 0x3a00_0000],
                    [0x3f90_0000_0000_0000, 0x3f40_0000_0000_0000],
                ),
                PhysicalCutoff::ThirtySevenHz => (
                    0x4214_0000,
                    0x4368_7a55,
                    0x4042_8000_0000_0000,
                    0x406d_0f4a_996e_d424,
                    [0x39dd_67c8, 0x375d_67c8],
                    [0x3f3b_acf9_14c1_bacf, 0x3eeb_acf9_14c1_bacf],
                ),
            };
            assert_eq!(
                f32_case.candidate_cutoff_bits(),
                ScalarBits::F32(expected_cutoff_f32)
            );
            assert_eq!(
                f32_case.candidate_angular_cutoff_bits(),
                ScalarBits::F32(expected_omega_f32)
            );
            assert_eq!(
                f64_case.candidate_cutoff_bits(),
                ScalarBits::F64(expected_cutoff_f64)
            );
            assert_eq!(
                f64_case.candidate_angular_cutoff_bits(),
                ScalarBits::F64(expected_omega_f64)
            );
            assert_eq!(
                duration_bits(&f32_case, 0),
                u64::from(expected_durations_f32[0])
            );
            assert_eq!(
                duration_bits(&f32_case, 1),
                u64::from(expected_durations_f32[1])
            );
            assert_eq!(duration_bits(&f64_case, 0), expected_durations_f64[0]);
            assert_eq!(duration_bits(&f64_case, 1), expected_durations_f64[1]);
        }
    }

    #[test]
    fn directed_rounding_holds_for_delta_one_and_thirty_two_on_every_axis() {
        for q_max in crate::Q_MAXES {
            let nominal = build_nominal_record(coordinate(
                TimeDomainFixture::GaussianPulse,
                q_max,
                crate::ScheduleKind::Uneven,
            ))
            .unwrap();
            let deltas = nominal
                .tick_positions()
                .windows(2)
                .map(|ticks| ticks[1] - ticks[0])
                .collect::<Vec<_>>();
            assert!(deltas.contains(&1));
            assert!(deltas.contains(&32));
            for scalar in SCALAR_ORDER {
                for cutoff in PHYSICAL_CUTOFFS {
                    let case = build_represented_case(&nominal, scalar, cutoff).unwrap();
                    for (index, delta) in deltas.iter().copied().enumerate() {
                        if delta != 1 && delta != 32 {
                            continue;
                        }
                        let q_i = reference_nominal_interval(delta, q_max.tick_denominator());
                        let quotient = reference_physical_duration(&q_i, cutoff.hz());
                        let duration = case.record().intervals_seconds()[index];
                        match scalar {
                            RecordScalar::F32 => {
                                let dt = duration as f32;
                                assert!(Float::with_val(REFERENCE_PRECISION, dt) <= quotient);
                                assert!(
                                    Float::with_val(REFERENCE_PRECISION, next_up_f32(dt))
                                        > quotient
                                );
                            }
                            RecordScalar::F64 => {
                                assert!(Float::with_val(REFERENCE_PRECISION, duration) <= quotient);
                                assert!(
                                    Float::with_val(REFERENCE_PRECISION, next_up_f64(duration))
                                        > quotient
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_frozen_shared_records_convert_for_both_scalars_and_cutoffs() {
        let mut conversion_count = 0;
        for fixture in TIME_DOMAIN_FIXTURES {
            for q_max in crate::Q_MAXES {
                for schedule in SCHEDULES {
                    let nominal =
                        build_nominal_record(coordinate(fixture, q_max, schedule)).unwrap();
                    for scalar in SCALAR_ORDER {
                        let one_hz =
                            build_represented_case(&nominal, scalar, PhysicalCutoff::OneHz)
                                .unwrap();
                        check_full_precision_geometry(&one_hz, &nominal);
                        let one_hz_samples = one_hz
                            .record()
                            .samples()
                            .iter()
                            .enumerate()
                            .map(|(index, _)| sample_bits(&one_hz, index))
                            .collect::<Vec<_>>();
                        drop(one_hz);

                        let thirty_seven_hz =
                            build_represented_case(&nominal, scalar, PhysicalCutoff::ThirtySevenHz)
                                .unwrap();
                        check_full_precision_geometry(&thirty_seven_hz, &nominal);
                        let thirty_seven_samples = thirty_seven_hz
                            .record()
                            .samples()
                            .iter()
                            .enumerate()
                            .map(|(index, _)| sample_bits(&thirty_seven_hz, index))
                            .collect::<Vec<_>>();
                        assert_eq!(one_hz_samples, thirty_seven_samples);
                        drop(thirty_seven_hz);
                        conversion_count += 2;
                    }
                }
            }
        }
        assert_eq!(conversion_count, 7 * 3 * 3 * 2 * 2);
    }

    #[test]
    fn repeated_conversion_is_exact_by_bits() {
        let nominal = build_nominal_record(coordinate(
            TimeDomainFixture::TaperedChirp,
            QMax::PiOver64,
            crate::ScheduleKind::Jitter,
        ))
        .unwrap();
        let first =
            build_represented_case(&nominal, RecordScalar::F32, PhysicalCutoff::ThirtySevenHz)
                .unwrap();
        let second =
            build_represented_case(&nominal, RecordScalar::F32, PhysicalCutoff::ThirtySevenHz)
                .unwrap();

        assert_eq!(
            first.candidate_cutoff_bits(),
            second.candidate_cutoff_bits()
        );
        assert_eq!(
            first.candidate_angular_cutoff_bits(),
            second.candidate_angular_cutoff_bits()
        );
        assert_eq!(
            first.record().cutoff_hz().to_bits(),
            second.record().cutoff_hz().to_bits()
        );
        assert_eq!(
            first
                .record()
                .intervals_seconds()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            second
                .record()
                .intervals_seconds()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .record()
                .samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            second
                .record()
                .samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }
}
