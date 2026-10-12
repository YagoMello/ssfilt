//! Exact candidate-scalar conversion for frozen nominal records.
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

use crate::{
    CaseCoordinate, FixtureCoordinate, NominalRecord, NominalToneRecord, RecordError, RecordScalar,
    RepresentedRecord, ToneFitMetadata,
};

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
    coordinate: CaseCoordinate,
    physical_cutoff: PhysicalCutoff,
    record: RepresentedRecord,
    candidate_cutoff_bits: ScalarBits,
    candidate_angular_cutoff_bits: ScalarBits,
    tone_fit_metadata: Option<ToneFitMetadata>,
}

impl RepresentedCase {
    #[must_use]
    pub const fn coordinate(&self) -> CaseCoordinate {
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

    /// The copied nominal fit metadata for tone cases; time-domain cases have none.
    #[must_use]
    pub const fn tone_fit_metadata(&self) -> Option<&ToneFitMetadata> {
        self.tone_fit_metadata.as_ref()
    }
}

/// A contextual failure while converting one nominal record for a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepresentedBuildError {
    pub coordinate: CaseCoordinate,
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
    ToneShapeMismatch {
        interval_count: usize,
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
    ToneFitWindowNotPositive,
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
            RepresentedBuildErrorKind::ToneShapeMismatch {
                interval_count,
                timestamp_count,
                sample_count,
            } => write!(
                formatter,
                "tone record shape has {interval_count} intervals, {timestamp_count} timestamps, and {sample_count} samples"
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
            RepresentedBuildErrorKind::ToneFitWindowNotPositive => formatter.write_str(
                "represented tone endpoint does not leave a positive guard-trimmed fit window",
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
    validate_time_domain_shape(nominal, scalar, cutoff)?;
    let intervals = time_domain_intervals(nominal, scalar, cutoff)?;
    build_conversion_kernel(
        ConversionCase::TimeDomain(coordinate),
        scalar,
        cutoff,
        &intervals,
        nominal.samples(),
    )
}

/// Convert exactly one nominal tone record to one frozen candidate scalar and
/// cutoff. The copied metadata remains attached to the shared case identity.
pub fn build_represented_tone_case(
    nominal: &NominalToneRecord,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
) -> Result<RepresentedCase, RepresentedBuildError> {
    let coordinate = nominal.coordinate();
    let intervals = nominal.normalized_intervals();
    let timestamp_count = nominal.normalized_times().len();
    let sample_count = nominal.samples().len();
    if intervals.is_empty()
        || intervals.len().checked_add(1) != Some(timestamp_count)
        || timestamp_count != sample_count
    {
        return Err(error(
            coordinate.into(),
            scalar,
            cutoff,
            RepresentedBuildErrorKind::ToneShapeMismatch {
                interval_count: intervals.len(),
                timestamp_count,
                sample_count,
            },
        ));
    }

    build_conversion_kernel(
        ConversionCase::Tone {
            coordinate,
            metadata: nominal.metadata().clone(),
        },
        scalar,
        cutoff,
        intervals,
        nominal.samples(),
    )
}

fn validate_time_domain_shape(
    nominal: &NominalRecord,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
) -> Result<(), RepresentedBuildError> {
    let tick_count = nominal.tick_positions().len();
    let interval_count = tick_count.saturating_sub(1);
    let timestamp_count = nominal.normalized_times().len();
    let sample_count = nominal.samples().len();
    if tick_count < 2 || tick_count != timestamp_count || tick_count != sample_count {
        return Err(error(
            nominal.coordinate().into(),
            scalar,
            cutoff,
            RepresentedBuildErrorKind::ShapeMismatch {
                tick_count,
                timestamp_count,
                sample_count,
            },
        ));
    }

    for index in 1..=interval_count {
        let previous = nominal.tick_positions()[index - 1];
        let current = nominal.tick_positions()[index];
        if current == previous {
            return Err(error(
                nominal.coordinate().into(),
                scalar,
                cutoff,
                RepresentedBuildErrorKind::ZeroTickDelta {
                    interval_index: index - 1,
                },
            ));
        }
        if current < previous {
            return Err(error(
                nominal.coordinate().into(),
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

fn time_domain_intervals(
    nominal: &NominalRecord,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
) -> Result<Vec<Float>, RepresentedBuildError> {
    let coordinate: CaseCoordinate = nominal.coordinate().into();
    let denominator = nominal.coordinate().q_max.tick_denominator();
    let mut intervals = Vec::with_capacity(nominal.tick_positions().len() - 1);
    for (interval_index, ticks) in nominal.tick_positions().windows(2).enumerate() {
        let delta = ticks[1].checked_sub(ticks[0]).ok_or_else(|| {
            error(
                coordinate,
                scalar,
                cutoff,
                RepresentedBuildErrorKind::TickDeltaOverflow { interval_index },
            )
        })?;
        if delta == 0 {
            return Err(error(
                coordinate,
                scalar,
                cutoff,
                RepresentedBuildErrorKind::ZeroTickDelta { interval_index },
            ));
        }
        intervals.push(nominal_interval(delta, denominator));
    }
    Ok(intervals)
}

/// Convert frozen normalized intervals and f64 samples through the exact
/// candidate-scalar path shared by time-domain and tone records.
fn build_conversion_kernel(
    source: ConversionCase,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
    normalized_intervals: &[Float],
    nominal_samples: &[f64],
) -> Result<RepresentedCase, RepresentedBuildError> {
    let coordinate = source.coordinate();
    let (cutoff_hz, candidate_cutoff_bits, candidate_angular_cutoff_bits) = match scalar {
        RecordScalar::F32 => {
            let cutoff_hz = cutoff.hz() as f32;
            if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
                return Err(error(
                    coordinate,
                    scalar,
                    cutoff,
                    RepresentedBuildErrorKind::InvalidRoundedCutoff,
                ));
            }
            let angular_cutoff = core::f32::consts::TAU * cutoff_hz;
            if !angular_cutoff.is_finite() || angular_cutoff <= 0.0 {
                return Err(error(
                    coordinate,
                    scalar,
                    cutoff,
                    RepresentedBuildErrorKind::InvalidAngularCutoff,
                ));
            }
            (
                f64::from(cutoff_hz),
                ScalarBits::F32(cutoff_hz.to_bits()),
                ScalarBits::F32(angular_cutoff.to_bits()),
            )
        }
        RecordScalar::F64 => {
            let cutoff_hz = cutoff.hz();
            if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
                return Err(error(
                    coordinate,
                    scalar,
                    cutoff,
                    RepresentedBuildErrorKind::InvalidRoundedCutoff,
                ));
            }
            let angular_cutoff = core::f64::consts::TAU * cutoff_hz;
            if !angular_cutoff.is_finite() || angular_cutoff <= 0.0 {
                return Err(error(
                    coordinate,
                    scalar,
                    cutoff,
                    RepresentedBuildErrorKind::InvalidAngularCutoff,
                ));
            }
            (
                cutoff_hz,
                ScalarBits::F64(cutoff_hz.to_bits()),
                ScalarBits::F64(angular_cutoff.to_bits()),
            )
        }
    };

    let mut intervals_seconds = Vec::with_capacity(normalized_intervals.len());
    for (interval_index, normalized_interval) in normalized_intervals.iter().enumerate() {
        let quotient = physical_duration_quotient(normalized_interval, cutoff_hz);
        let duration = match scalar {
            RecordScalar::F32 => f64::from(quotient.to_f32_round(Round::Down)),
            RecordScalar::F64 => quotient.to_f64_round(Round::Down),
        };
        if !duration.is_finite() {
            return Err(error(
                coordinate,
                scalar,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteDuration { interval_index },
            ));
        }
        if duration <= 0.0 {
            return Err(error(
                coordinate,
                scalar,
                cutoff,
                RepresentedBuildErrorKind::NonPositiveDuration { interval_index },
            ));
        }
        intervals_seconds.push(duration);
    }

    let mut samples = Vec::with_capacity(nominal_samples.len());
    for (sample_index, sample) in nominal_samples.iter().copied().enumerate() {
        let rounded = match scalar {
            RecordScalar::F32 => f64::from(sample as f32),
            RecordScalar::F64 => sample,
        };
        if !rounded.is_finite() {
            return Err(error(
                coordinate,
                scalar,
                cutoff,
                RepresentedBuildErrorKind::NonFiniteRoundedSample { sample_index },
            ));
        }
        samples.push(rounded);
    }

    let record = match scalar {
        RecordScalar::F32 => RepresentedRecord::from_f32(
            cutoff_hz as f32,
            intervals_seconds
                .iter()
                .map(|value| *value as f32)
                .collect(),
            samples.iter().map(|value| *value as f32).collect(),
        ),
        RecordScalar::F64 => RepresentedRecord::from_f64(cutoff_hz, intervals_seconds, samples),
    }
    .map_err(|source| {
        error(
            coordinate,
            scalar,
            cutoff,
            RepresentedBuildErrorKind::Record(source),
        )
    })?;

    if let Some(metadata) = source.tone_fit_metadata()
        && !has_positive_represented_fit_window(&record, metadata)
    {
        return Err(error(
            coordinate,
            scalar,
            cutoff,
            RepresentedBuildErrorKind::ToneFitWindowNotPositive,
        ));
    }

    Ok(RepresentedCase {
        coordinate,
        physical_cutoff: cutoff,
        record,
        candidate_cutoff_bits,
        candidate_angular_cutoff_bits,
        tone_fit_metadata: source.tone_fit_metadata().cloned(),
    })
}

#[derive(Clone, Debug)]
enum ConversionCase {
    TimeDomain(FixtureCoordinate),
    Tone {
        coordinate: crate::ToneCoordinate,
        metadata: ToneFitMetadata,
    },
}

impl ConversionCase {
    const fn coordinate(&self) -> CaseCoordinate {
        match self {
            Self::TimeDomain(coordinate) => CaseCoordinate::TimeDomain(*coordinate),
            Self::Tone { coordinate, .. } => CaseCoordinate::Tone(*coordinate),
        }
    }

    fn tone_fit_metadata(&self) -> Option<&ToneFitMetadata> {
        match self {
            Self::TimeDomain(_) => None,
            Self::Tone { metadata, .. } => Some(metadata),
        }
    }
}

fn has_positive_represented_fit_window(
    record: &RepresentedRecord,
    metadata: &ToneFitMetadata,
) -> bool {
    let mut factor = Float::with_val(CONVERSION_PRECISION, Constant::Pi);
    factor *= 2;
    factor *= Float::with_val(CONVERSION_PRECISION, record.cutoff_hz());

    let mut endpoint = Float::with_val(CONVERSION_PRECISION, 0);
    for duration in record.intervals_seconds() {
        let mut normalized_interval = factor.clone();
        normalized_interval *= Float::with_val(CONVERSION_PRECISION, *duration);
        endpoint += normalized_interval;
    }

    endpoint -= metadata.guard();
    &endpoint > metadata.guard()
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
    coordinate: CaseCoordinate,
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
    use crate::{
        QMax, SCHEDULES, TIME_DOMAIN_FIXTURES, TONE_FREQUENCIES, TONE_ORDERS, TONE_PHASES,
        TONE_RECORD_COUNT, TimeDomainFixture, ToneFrequency, ToneOrder, TonePhase,
        build_nominal_record, build_nominal_tone_record, build_represented_tone_case,
    };

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

    fn represented_q(case: &RepresentedCase, interval_index: usize) -> Float {
        let mut q_hat = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        q_hat *= 2;
        q_hat *= Float::with_val(REFERENCE_PRECISION, case.record.cutoff_hz());
        q_hat *= Float::with_val(
            REFERENCE_PRECISION,
            case.record.intervals_seconds()[interval_index],
        );
        q_hat
    }

    fn represented_tone_endpoint(case: &RepresentedCase) -> Float {
        let mut endpoint = Float::with_val(REFERENCE_PRECISION, 0);
        for interval_index in 0..case.record.intervals_seconds().len() {
            endpoint += represented_q(case, interval_index);
        }
        endpoint
    }

    fn assert_tone_conversion(
        case: &RepresentedCase,
        nominal: &crate::NominalToneRecord,
        scalar: RecordScalar,
        cutoff: PhysicalCutoff,
    ) {
        assert_eq!(
            case.coordinate(),
            CaseCoordinate::Tone(nominal.coordinate())
        );
        assert_eq!(case.physical_cutoff(), cutoff);
        assert_eq!(case.tone_fit_metadata(), Some(nominal.metadata()));
        assert_eq!(case.record.scalar(), scalar);
        assert_eq!(case.record.samples().len(), nominal.samples().len());
        assert_eq!(
            case.record.intervals_seconds().len(),
            nominal.normalized_intervals().len()
        );

        let expected_cutoff_bits = match scalar {
            RecordScalar::F32 => ScalarBits::F32((cutoff.hz() as f32).to_bits()),
            RecordScalar::F64 => ScalarBits::F64(cutoff.hz().to_bits()),
        };
        let expected_angular_bits = match scalar {
            RecordScalar::F32 => {
                let cutoff_hz = cutoff.hz() as f32;
                ScalarBits::F32((core::f32::consts::TAU * cutoff_hz).to_bits())
            }
            RecordScalar::F64 => ScalarBits::F64((core::f64::consts::TAU * cutoff.hz()).to_bits()),
        };
        assert_eq!(case.candidate_cutoff_bits(), expected_cutoff_bits);
        assert_eq!(case.candidate_angular_cutoff_bits(), expected_angular_bits);

        let mut q_max = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        q_max /= nominal.coordinate().q_max.denominator();
        for (interval_index, q_i) in nominal.normalized_intervals().iter().enumerate() {
            let dt = case.record.intervals_seconds()[interval_index];
            let quotient = reference_physical_duration(q_i, case.record.cutoff_hz());
            let q_hat = represented_q(case, interval_index);
            assert!(&q_hat <= q_i, "{scalar:?} {cutoff:?} q at {interval_index}");
            assert!(q_i <= &q_max, "nominal q at {interval_index}");

            match scalar {
                RecordScalar::F32 => {
                    let represented = dt as f32;
                    let next = next_up_f32(represented);
                    assert!(Float::with_val(REFERENCE_PRECISION, represented) <= quotient);
                    assert!(Float::with_val(REFERENCE_PRECISION, next) > quotient);
                }
                RecordScalar::F64 => {
                    let next = next_up_f64(dt);
                    assert!(Float::with_val(REFERENCE_PRECISION, dt) <= quotient);
                    assert!(Float::with_val(REFERENCE_PRECISION, next) > quotient);
                }
            }

            let expected_sample_bits = match scalar {
                RecordScalar::F32 => (nominal.samples()[interval_index] as f32).to_bits() as u64,
                RecordScalar::F64 => nominal.samples()[interval_index].to_bits(),
            };
            assert_eq!(sample_bits(case, interval_index), expected_sample_bits);
        }
        let last_index = nominal.samples().len() - 1;
        let expected_last_bits = match scalar {
            RecordScalar::F32 => (nominal.samples()[last_index] as f32).to_bits() as u64,
            RecordScalar::F64 => nominal.samples()[last_index].to_bits(),
        };
        assert_eq!(sample_bits(case, last_index), expected_last_bits);

        let mut trimmed_end = represented_tone_endpoint(case);
        trimmed_end -= nominal.metadata().guard();
        assert!(&trimmed_end > nominal.metadata().guard());
    }

    fn check_full_precision_geometry(case: &RepresentedCase, nominal: &NominalRecord) {
        assert_eq!(
            case.coordinate(),
            CaseCoordinate::from(nominal.coordinate())
        );
        assert!(case.tone_fit_metadata().is_none());
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

    #[test]
    fn represented_matrix_arithmetic_is_frozen() {
        let time_domain_cells = 7 * 3 * 3 * 2 * 2 * 4;
        let tone_cells = TONE_RECORD_COUNT * 2 * 2;
        assert_eq!(time_domain_cells, 1_008);
        assert_eq!(tone_cells, 1_440);
        assert_eq!(time_domain_cells + tone_cells, 2_448);
    }

    #[test]
    fn all_tone_represented_cells_preserve_rounding_bits_and_fit_windows() {
        let mut converted_cells = 0;
        let mut largest_shape = (0_usize, 0_usize);
        let mut largest_coordinate = None;

        for order in TONE_ORDERS {
            for frequency in TONE_FREQUENCIES {
                for phase in TONE_PHASES {
                    for q_max in crate::Q_MAXES {
                        for schedule in SCHEDULES {
                            let nominal_coordinate = crate::ToneCoordinate {
                                order,
                                frequency,
                                phase,
                                q_max,
                                schedule,
                            };
                            let nominal = build_nominal_tone_record(nominal_coordinate).unwrap();
                            let shape = (
                                nominal.normalized_intervals().len(),
                                nominal.samples().len(),
                            );
                            if shape.1 > largest_shape.1 {
                                largest_shape = shape;
                                largest_coordinate = Some(nominal_coordinate);
                            }

                            for scalar in SCALAR_ORDER {
                                let one_hz = build_represented_tone_case(
                                    &nominal,
                                    scalar,
                                    PhysicalCutoff::OneHz,
                                )
                                .unwrap();
                                assert_tone_conversion(
                                    &one_hz,
                                    &nominal,
                                    scalar,
                                    PhysicalCutoff::OneHz,
                                );
                                let one_hz_samples = (0..one_hz.record().samples().len())
                                    .map(|index| sample_bits(&one_hz, index))
                                    .collect::<Vec<_>>();
                                drop(one_hz);

                                let thirty_seven_hz = build_represented_tone_case(
                                    &nominal,
                                    scalar,
                                    PhysicalCutoff::ThirtySevenHz,
                                )
                                .unwrap();
                                assert_tone_conversion(
                                    &thirty_seven_hz,
                                    &nominal,
                                    scalar,
                                    PhysicalCutoff::ThirtySevenHz,
                                );
                                let thirty_seven_hz_samples =
                                    (0..thirty_seven_hz.record().samples().len())
                                        .map(|index| sample_bits(&thirty_seven_hz, index))
                                        .collect::<Vec<_>>();
                                assert_eq!(one_hz_samples, thirty_seven_hz_samples);
                                drop(thirty_seven_hz);
                                converted_cells += 2;
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(converted_cells, 1_440);
        assert_eq!(largest_shape, (33_013, 33_014));
        assert_eq!(
            largest_coordinate,
            Some(crate::ToneCoordinate {
                order: ToneOrder::Eight,
                frequency: ToneFrequency::OneSixteenth,
                phase: TonePhase::SeventeenHundredths,
                q_max: QMax::PiOver128,
                schedule: crate::ScheduleKind::Uneven,
            })
        );
    }

    #[test]
    fn tone_represented_conversion_is_deterministic_and_error_display_keeps_context() {
        let nominal = build_nominal_tone_record(crate::ToneCoordinate {
            order: ToneOrder::Four,
            frequency: ToneFrequency::SevenTenths,
            phase: TonePhase::OneNineteen,
            q_max: QMax::PiOver64,
            schedule: crate::ScheduleKind::Jitter,
        })
        .unwrap();
        let first =
            build_represented_tone_case(&nominal, RecordScalar::F32, PhysicalCutoff::ThirtySevenHz)
                .unwrap();
        let second =
            build_represented_tone_case(&nominal, RecordScalar::F32, PhysicalCutoff::ThirtySevenHz)
                .unwrap();
        assert_eq!(first.coordinate(), second.coordinate());
        assert_eq!(first.tone_fit_metadata(), second.tone_fit_metadata());
        assert_eq!(
            first.candidate_cutoff_bits(),
            second.candidate_cutoff_bits()
        );
        assert_eq!(
            first.candidate_angular_cutoff_bits(),
            second.candidate_angular_cutoff_bits()
        );
        assert_eq!(
            first.record().intervals_seconds(),
            second.record().intervals_seconds()
        );
        assert_eq!(first.record().samples(), second.record().samples());

        let contextual = error(
            CaseCoordinate::Tone(nominal.coordinate()),
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            RepresentedBuildErrorKind::ToneFitWindowNotPositive,
        );
        assert!(contextual.to_string().contains("Tone("));
        assert!(
            contextual
                .to_string()
                .contains("positive guard-trimmed fit window")
        );
    }

    fn tone_remainder_witness_coordinate() -> crate::ToneCoordinate {
        crate::ToneCoordinate {
            order: ToneOrder::One,
            frequency: ToneFrequency::OneSixteenth,
            phase: TonePhase::SeventeenHundredths,
            q_max: QMax::PiOver128,
            schedule: crate::ScheduleKind::Uniform,
        }
    }

    fn terminal_full_tick_tau(nominal: &crate::NominalToneRecord) -> Float {
        let mut tau = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        tau *= nominal.full_tick_count();
        tau /= nominal.coordinate().q_max.tick_denominator();
        tau
    }

    fn scalar_cutoff_hz(scalar: RecordScalar, cutoff: PhysicalCutoff) -> f64 {
        match scalar {
            RecordScalar::F32 => f64::from(cutoff.hz() as f32),
            RecordScalar::F64 => cutoff.hz(),
        }
    }

    fn round_down_bits(value: &Float, scalar: RecordScalar) -> u64 {
        match scalar {
            RecordScalar::F32 => value.to_f32_round(Round::Down).to_bits() as u64,
            RecordScalar::F64 => value.to_f64_round(Round::Down).to_bits(),
        }
    }

    fn assert_downward_maximality(quotient: &Float, scalar: RecordScalar, bits: u64) {
        match scalar {
            RecordScalar::F32 => {
                let duration = f32::from_bits(bits as u32);
                let represented = Float::with_val(REFERENCE_PRECISION, duration);
                let next = Float::with_val(REFERENCE_PRECISION, next_up_f32(duration));
                assert!(&represented <= quotient);
                assert!(&next > quotient);
            }
            RecordScalar::F64 => {
                let duration = f64::from_bits(bits);
                let represented = Float::with_val(REFERENCE_PRECISION, duration);
                let next = Float::with_val(REFERENCE_PRECISION, next_up_f64(duration));
                assert!(&represented <= quotient);
                assert!(&next > quotient);
            }
        }
    }

    #[test]
    fn frozen_terminal_remainder_uses_stored_q() {
        const TERMINAL_Q_BINARY: &str = "1.000011110100110011010000000010011001100101101100001101010010100000010111100110101010100101000000000101100001110101110100111100111000001010001001111111111000101100000000010110110111010011010000010110100111101000000000010011100001101101101101010000000000000e-6";
        const DURATION_CASES: [(RecordScalar, PhysicalCutoff, u64, u64); 4] = [
            (
                RecordScalar::F32,
                PhysicalCutoff::OneHz,
                0x3b2c_b708,
                0x3b2c_b708,
            ),
            (
                RecordScalar::F32,
                PhysicalCutoff::ThirtySevenHz,
                0x3895_6007,
                0x3895_6007,
            ),
            (
                RecordScalar::F64,
                PhysicalCutoff::OneHz,
                0x3f65_96e1_14b4_1a91,
                0x3f65_96e1_14b4_2654,
            ),
            (
                RecordScalar::F64,
                PhysicalCutoff::ThirtySevenHz,
                0x3f12_ac00_ef4f_a846,
                0x3f12_ac00_ef4f_b272,
            ),
        ];

        let nominal = build_nominal_tone_record(tone_remainder_witness_coordinate()).unwrap();
        assert_eq!(
            nominal.normalized_intervals().len(),
            nominal.full_interval_count() + 1
        );
        let terminal_q = nominal.normalized_intervals().last().unwrap();
        assert_eq!(terminal_q.prec(), REFERENCE_PRECISION);
        assert_eq!(terminal_q.to_string_radix(2, None), TERMINAL_Q_BINARY);

        let last_full_tick_tau = terminal_full_tick_tau(&nominal);
        let mut endpoint_remainder = nominal.metadata().nominal_endpoint().clone();
        endpoint_remainder -= last_full_tick_tau;
        assert_eq!(&endpoint_remainder, terminal_q);

        let timestamps = nominal.normalized_times();
        let timestamp_delta = timestamps[timestamps.len() - 1] - timestamps[timestamps.len() - 2];
        let timestamp_q = Float::with_val(REFERENCE_PRECISION, timestamp_delta);
        assert_ne!(terminal_q, &timestamp_q);

        for (scalar, cutoff, expected_bits, timestamp_derived_bits) in DURATION_CASES {
            let cutoff_hz = scalar_cutoff_hz(scalar, cutoff);
            let quotient = physical_duration_quotient(terminal_q, cutoff_hz);
            let timestamp_derived_quotient = physical_duration_quotient(&timestamp_q, cutoff_hz);
            assert_eq!(round_down_bits(&quotient, scalar), expected_bits);
            assert_eq!(
                round_down_bits(&timestamp_derived_quotient, scalar),
                timestamp_derived_bits
            );
            assert_downward_maximality(&quotient, scalar, expected_bits);

            let represented = build_represented_tone_case(&nominal, scalar, cutoff).unwrap();
            let actual_bits = duration_bits(
                &represented,
                represented.record().intervals_seconds().len() - 1,
            );
            assert_eq!(actual_bits, expected_bits);
        }
    }

    #[test]
    fn frozen_terminal_remainder_rearrangement_changes_only_256_bit_quotient() {
        const QUOTIENT_BINARY: &str = "1.010110010110111000010001010010110100000110101001000111110111101010110011000010001010010011101011111111110101111011001100000010000011011100110110111010100111100101111100010001000000100111110001000111101101011110111100011100000110111011001000010100010011100e-9";
        const REARRANGED_BINARY: &str = "1.010110010110111000010001010010110100000110101001000111110111101010110011000010001010010011101011111111110101111011001100000010000011011100110110111010100111100101111100010001000000100111110001000111101101011110111100011100000110111011001000100000000000000e-9";

        let nominal = build_nominal_tone_record(tone_remainder_witness_coordinate()).unwrap();
        let terminal_q = nominal.normalized_intervals().last().unwrap();
        let endpoint = nominal.metadata().nominal_endpoint();
        let last_full_tick_tau = terminal_full_tick_tau(&nominal);
        let cutoff = PhysicalCutoff::OneHz;

        let mut denominator = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        denominator *= 2;
        denominator *= Float::with_val(REFERENCE_PRECISION, cutoff.hz());

        let mut quotient = terminal_q.clone();
        quotient /= &denominator;
        let mut rearranged = Float::with_val(REFERENCE_PRECISION, endpoint);
        rearranged /= &denominator;
        let mut last_tick_quotient = last_full_tick_tau;
        last_tick_quotient /= denominator;
        rearranged -= last_tick_quotient;

        assert_eq!(quotient.to_string_radix(2, None), QUOTIENT_BINARY);
        assert_eq!(rearranged.to_string_radix(2, None), REARRANGED_BINARY);
        assert_ne!(quotient, rearranged);

        for (scalar, expected_bits) in [
            (RecordScalar::F32, 0x3b2c_b708),
            (RecordScalar::F64, 0x3f65_96e1_14b4_1a91),
        ] {
            assert_eq!(round_down_bits(&quotient, scalar), expected_bits);
            assert_eq!(round_down_bits(&rearranged, scalar), expected_bits);
        }
    }

    #[test]
    fn frozen_full_tick_cancellation_changes_only_256_bit_quotient() {
        const PRESCRIBED_BINARY: &str = "1.100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001e-9";
        const CANCELED_BINARY: &str = "1.100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e-9";

        let q_max = QMax::PiOver128;
        let schedule = crate::ScheduleKind::Jitter;
        let delta = 24_u64;
        assert!(schedule.tick_pattern().contains(&delta));

        let prescribed_q = nominal_interval(delta, q_max.tick_denominator());
        let prescribed = physical_duration_quotient(&prescribed_q, PhysicalCutoff::OneHz.hz());

        let mut canceled_denominator =
            Float::with_val(REFERENCE_PRECISION, q_max.tick_denominator() * 2);
        canceled_denominator *= Float::with_val(REFERENCE_PRECISION, PhysicalCutoff::OneHz.hz());
        let mut canceled = Float::with_val(REFERENCE_PRECISION, delta);
        canceled /= canceled_denominator;

        assert_eq!(prescribed.to_string_radix(2, None), PRESCRIBED_BINARY);
        assert_eq!(canceled.to_string_radix(2, None), CANCELED_BINARY);
        assert_ne!(prescribed, canceled);

        for (scalar, expected_bits) in [
            (RecordScalar::F32, 0x3b40_0000),
            (RecordScalar::F64, 0x3f68_0000_0000_0000),
        ] {
            assert_eq!(round_down_bits(&prescribed, scalar), expected_bits);
            assert_eq!(round_down_bits(&canceled, scalar), expected_bits);
        }
    }
}
