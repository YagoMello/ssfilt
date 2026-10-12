//! Frozen nominal tone records for the forward-backward response matrix.
//!
//! Tone schedules use exact integer ticks for full intervals and 256-bit
//! round-to-nearest values for normalized intervals, metadata, and endpoints.
//! Absolute normalized timestamps are converted independently to `f64` only
//! after the complete high-precision schedule has been fixed.

use core::fmt;

use rug::{Float, float::Constant};

use crate::{QMax, ScheduleKind};

const TONE_PRECISION: u32 = 256;

/// Stable declaration order for the nominal per-pass tone orders.
pub const TONE_ORDERS: [ToneOrder; 4] = [
    ToneOrder::One,
    ToneOrder::Two,
    ToneOrder::Four,
    ToneOrder::Eight,
];

/// Stable declaration order for the nominal tone-frequency ratios.
pub const TONE_FREQUENCIES: [ToneFrequency; 5] = [
    ToneFrequency::OneSixteenth,
    ToneFrequency::OneQuarter,
    ToneFrequency::SevenTenths,
    ToneFrequency::One,
    ToneFrequency::Two,
];

/// Stable declaration order for the nominal tone phases.
pub const TONE_PHASES: [TonePhase; 2] = [TonePhase::SeventeenHundredths, TonePhase::OneNineteen];

/// The explicitly frozen number of nominal tone records.
pub const TONE_RECORD_COUNT: usize = 360;

/// One of the four per-pass Butterworth orders in the tone matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneOrder {
    One,
    Two,
    Four,
    Eight,
}

impl ToneOrder {
    /// Return the integer order used by the nominal compensated response.
    #[must_use]
    pub const fn value(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

/// One of the five exact rational normalized-frequency ratios.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneFrequency {
    OneSixteenth,
    OneQuarter,
    SevenTenths,
    One,
    Two,
}

impl ToneFrequency {
    /// Return the exact integer numerator and denominator of this axis value.
    #[must_use]
    pub const fn ratio(self) -> (u32, u32) {
        match self {
            Self::OneSixteenth => (1, 16),
            Self::OneQuarter => (1, 4),
            Self::SevenTenths => (7, 10),
            Self::One => (1, 1),
            Self::Two => (2, 1),
        }
    }

    /// Evaluate this exact integer ratio once as an `f64` for tone sampling.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        let (numerator, denominator) = self.ratio();
        f64::from(numerator) / f64::from(denominator)
    }
}

/// One of the two frozen tone phases, in radians.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TonePhase {
    SeventeenHundredths,
    OneNineteen,
}

impl TonePhase {
    /// Return the frozen phase rounded to `f64`.
    #[must_use]
    pub const fn radians(self) -> f64 {
        match self {
            Self::SeventeenHundredths => 0.17,
            Self::OneNineteen => 1.19,
        }
    }
}

/// The coordinate identifying one nominal tone record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToneCoordinate {
    pub order: ToneOrder,
    pub frequency: ToneFrequency,
    pub phase: TonePhase,
    pub q_max: QMax,
    pub schedule: ScheduleKind,
}

/// Frozen high-precision values for the tone's nominal fit window.
#[derive(Clone, Debug, PartialEq)]
pub struct ToneFitMetadata {
    guard: Float,
    period: Float,
    nominal_endpoint: Float,
}

impl ToneFitMetadata {
    /// The nominal one-sided edge guard, `G = 24 / gamma_N`.
    #[must_use]
    pub const fn guard(&self) -> &Float {
        &self.guard
    }

    /// The nominal tone period, `P = (2*pi) / W`.
    #[must_use]
    pub const fn period(&self) -> &Float {
        &self.period
    }

    /// The frozen record endpoint, `2*G + P`.
    #[must_use]
    pub const fn nominal_endpoint(&self) -> &Float {
        &self.nominal_endpoint
    }
}

/// One complete, frozen nominal record for a tone matrix coordinate.
#[derive(Clone, Debug, PartialEq)]
pub struct NominalToneRecord {
    coordinate: ToneCoordinate,
    normalized_intervals: Vec<Float>,
    normalized_times: Vec<f64>,
    samples: Vec<f64>,
    metadata: ToneFitMetadata,
    full_tick_count: u64,
    full_interval_count: usize,
}

impl NominalToneRecord {
    #[must_use]
    pub const fn coordinate(&self) -> ToneCoordinate {
        self.coordinate
    }

    /// The 256-bit round-to-nearest normalized intervals, including a final
    /// positive remainder when the endpoint does not close on an exact tick.
    #[must_use]
    pub fn normalized_intervals(&self) -> &[Float] {
        &self.normalized_intervals
    }

    /// Independently rounded `f64` normalized timestamps, including both ends.
    #[must_use]
    pub fn normalized_times(&self) -> &[f64] {
        &self.normalized_times
    }

    /// Analytic samples evaluated once from each stored `f64` timestamp.
    #[must_use]
    pub fn samples(&self) -> &[f64] {
        &self.samples
    }

    #[must_use]
    pub const fn metadata(&self) -> &ToneFitMetadata {
        &self.metadata
    }

    /// The tick position reached by the last complete rational-tick interval.
    #[must_use]
    pub const fn full_tick_count(&self) -> u64 {
        self.full_tick_count
    }

    /// The number of complete rational-tick intervals before any remainder.
    #[must_use]
    pub const fn full_interval_count(&self) -> usize {
        self.full_interval_count
    }
}

/// Which record vector could not reserve its checked capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneRecordBuffer {
    Intervals,
    Timestamps,
    Samples,
}

/// Contextual failure while constructing one nominal tone record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneBuildError {
    coordinate: ToneCoordinate,
    kind: ToneBuildErrorKind,
}

impl ToneBuildError {
    #[must_use]
    pub const fn coordinate(&self) -> ToneCoordinate {
        self.coordinate
    }

    #[must_use]
    pub const fn kind(&self) -> &ToneBuildErrorKind {
        &self.kind
    }
}

/// Cause of a contextual tone-record construction failure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToneBuildErrorKind {
    TickCountOverflow {
        interval_index: usize,
    },
    IntervalCountOverflow {
        interval_index: usize,
    },
    CapacityOverflow {
        buffer: ToneRecordBuffer,
    },
    CapacityReservationFailed {
        buffer: ToneRecordBuffer,
        requested_capacity: usize,
    },
    InvalidMetadata,
    InvalidFullInterval {
        interval_index: usize,
        value: f64,
    },
    InvalidRemainder {
        interval_index: usize,
        value: f64,
    },
    NonFiniteTimestamp {
        index: usize,
    },
    NonIncreasingTimestamp {
        index: usize,
        previous: f64,
        current: f64,
    },
    NonFiniteSample {
        index: usize,
        value: f64,
    },
}

impl fmt::Display for ToneBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "tone record build failed for {:?}: ",
            self.coordinate
        )?;
        match self.kind {
            ToneBuildErrorKind::TickCountOverflow { interval_index } => write!(
                formatter,
                "tick count overflow at interval {interval_index}"
            ),
            ToneBuildErrorKind::IntervalCountOverflow { interval_index } => write!(
                formatter,
                "interval count overflow at interval {interval_index}"
            ),
            ToneBuildErrorKind::CapacityOverflow { buffer } => {
                write!(formatter, "capacity arithmetic overflow for {buffer:?}")
            }
            ToneBuildErrorKind::CapacityReservationFailed {
                buffer,
                requested_capacity,
            } => write!(
                formatter,
                "could not reserve {requested_capacity} entries for {buffer:?}"
            ),
            ToneBuildErrorKind::InvalidMetadata => {
                formatter.write_str("tone metadata is not finite and positive")
            }
            ToneBuildErrorKind::InvalidFullInterval {
                interval_index,
                value,
            } => write!(
                formatter,
                "full interval {interval_index} is not finite, positive, and within q_max ({value})"
            ),
            ToneBuildErrorKind::InvalidRemainder {
                interval_index,
                value,
            } => write!(
                formatter,
                "remainder interval {interval_index} is not finite, positive, and within q_max ({value})"
            ),
            ToneBuildErrorKind::NonFiniteTimestamp { index } => {
                write!(formatter, "timestamp {index} is not finite")
            }
            ToneBuildErrorKind::NonIncreasingTimestamp {
                index,
                previous,
                current,
            } => write!(
                formatter,
                "timestamp {index} ({current}) is not greater than its predecessor ({previous})"
            ),
            ToneBuildErrorKind::NonFiniteSample { index, value } => {
                write!(formatter, "sample {index} ({value}) is not finite")
            }
        }
    }
}

impl std::error::Error for ToneBuildError {}

/// Build the nominal record for one tone matrix coordinate.
///
/// Only this coordinate's record is materialized. Exact integer ticks and the
/// two scalar tick summaries are sufficient to reconstruct its schedule; no
/// redundant per-timestamp tick vector is retained.
pub fn build_nominal_tone_record(
    coordinate: ToneCoordinate,
) -> Result<NominalToneRecord, ToneBuildError> {
    let metadata = tone_metadata(coordinate)?;

    let endpoint = metadata.nominal_endpoint().clone();
    let precision = TONE_PRECISION;
    let pi = Float::with_val(precision, Constant::Pi);
    let mut q_max = Float::with_val(precision, &pi);
    q_max /= coordinate.q_max.denominator();
    let tick_denominator = coordinate.q_max.tick_denominator();
    let pattern = coordinate.schedule.tick_pattern();

    let mut normalized_intervals = Vec::new();
    let mut normalized_times = Vec::new();
    let mut full_tick_count = 0_u64;
    let mut full_interval_count = 0_usize;

    try_push(
        &mut normalized_times,
        0.0,
        coordinate,
        ToneRecordBuffer::Timestamps,
    )?;

    loop {
        let interval_index = normalized_intervals.len();
        let pattern_index = full_interval_count % pattern.len();
        let delta = pattern[pattern_index];
        let candidate_ticks = full_tick_count.checked_add(delta).ok_or_else(|| {
            error(
                coordinate,
                ToneBuildErrorKind::TickCountOverflow { interval_index },
            )
        })?;
        let candidate_tau = tick_timestamp(&pi, candidate_ticks, tick_denominator);

        if candidate_tau <= endpoint {
            let interval = tick_interval(&pi, delta, tick_denominator);
            if !valid_interval(&interval, &q_max) {
                return Err(error(
                    coordinate,
                    ToneBuildErrorKind::InvalidFullInterval {
                        interval_index,
                        value: interval.to_f64(),
                    },
                ));
            }
            try_push(
                &mut normalized_intervals,
                interval,
                coordinate,
                ToneRecordBuffer::Intervals,
            )?;
            full_tick_count = candidate_ticks;
            full_interval_count = full_interval_count.checked_add(1).ok_or_else(|| {
                error(
                    coordinate,
                    ToneBuildErrorKind::IntervalCountOverflow { interval_index },
                )
            })?;

            let timestamp = candidate_tau.to_f64();
            push_timestamp(&mut normalized_times, timestamp, coordinate)?;
            if candidate_tau == endpoint {
                break;
            }
        } else {
            // Recompute the prior absolute tick timestamp directly. Do not
            // recover it by summing separately rounded interval values.
            let last_tau = tick_timestamp(&pi, full_tick_count, tick_denominator);
            let mut remainder = endpoint.clone();
            remainder -= last_tau;
            if !valid_interval(&remainder, &q_max) {
                return Err(error(
                    coordinate,
                    ToneBuildErrorKind::InvalidRemainder {
                        interval_index,
                        value: remainder.to_f64(),
                    },
                ));
            }
            try_push(
                &mut normalized_intervals,
                remainder,
                coordinate,
                ToneRecordBuffer::Intervals,
            )?;
            push_timestamp(&mut normalized_times, endpoint.to_f64(), coordinate)?;
            break;
        }
    }

    let frequency_f64 = coordinate.frequency.as_f64();
    let phase_f64 = coordinate.phase.radians();
    let mut samples = Vec::new();
    for (index, timestamp) in normalized_times.iter().copied().enumerate() {
        let value = (frequency_f64 * timestamp + phase_f64).cos();
        if !value.is_finite() {
            return Err(error(
                coordinate,
                ToneBuildErrorKind::NonFiniteSample { index, value },
            ));
        }
        try_push(&mut samples, value, coordinate, ToneRecordBuffer::Samples)?;
    }

    Ok(NominalToneRecord {
        coordinate,
        normalized_intervals,
        normalized_times,
        samples,
        metadata,
        full_tick_count,
        full_interval_count,
    })
}

fn tone_metadata(coordinate: ToneCoordinate) -> Result<ToneFitMetadata, ToneBuildError> {
    let precision = TONE_PRECISION;
    let order = coordinate.order.value();

    let mut sqrt_two = Float::with_val(precision, 2);
    sqrt_two.sqrt_mut();
    let mut root = sqrt_two.clone();
    root -= 1;

    let mut exponent = Float::with_val(precision, -1);
    exponent /= 2 * order;
    let mut logarithm = root;
    logarithm.ln_mut();
    let mut exponent_product = logarithm;
    exponent_product *= exponent;
    let mut alpha = exponent_product;
    alpha.exp_mut();

    let pi = Float::with_val(precision, Constant::Pi);
    let mut angle = pi.clone();
    angle /= 2 * order;
    let mut sine = angle;
    sine.sin_mut();
    let mut gamma = alpha.clone();
    gamma *= sine;

    let mut guard = Float::with_val(precision, 24);
    guard /= &gamma;

    let (frequency_numerator, frequency_denominator) = coordinate.frequency.ratio();
    let mut frequency = Float::with_val(precision, frequency_numerator);
    frequency /= frequency_denominator;
    let mut two_pi = pi;
    two_pi *= 2;
    let mut period = two_pi;
    period /= &frequency;

    let mut nominal_endpoint = guard.clone();
    nominal_endpoint *= 2;
    nominal_endpoint += &period;

    if !alpha.is_finite()
        || alpha <= 0
        || !gamma.is_finite()
        || gamma <= 0
        || !guard.is_finite()
        || guard <= 0
        || !frequency.is_finite()
        || frequency <= 0
        || !period.is_finite()
        || period <= 0
        || !nominal_endpoint.is_finite()
        || nominal_endpoint <= 0
    {
        return Err(error(coordinate, ToneBuildErrorKind::InvalidMetadata));
    }

    Ok(ToneFitMetadata {
        guard,
        period,
        nominal_endpoint,
    })
}

fn tick_interval(pi: &Float, ticks: u64, tick_denominator: u64) -> Float {
    let mut interval = pi.clone();
    interval *= ticks;
    interval /= tick_denominator;
    interval
}

fn tick_timestamp(pi: &Float, ticks: u64, tick_denominator: u64) -> Float {
    let mut timestamp = pi.clone();
    timestamp *= ticks;
    timestamp /= tick_denominator;
    timestamp
}

fn valid_interval(interval: &Float, q_max: &Float) -> bool {
    interval.is_finite() && interval > &0 && interval <= q_max
}

fn push_timestamp(
    timestamps: &mut Vec<f64>,
    timestamp: f64,
    coordinate: ToneCoordinate,
) -> Result<(), ToneBuildError> {
    let index = timestamps.len();
    if !timestamp.is_finite() {
        return Err(error(
            coordinate,
            ToneBuildErrorKind::NonFiniteTimestamp { index },
        ));
    }
    if let Some(previous) = timestamps.last().copied()
        && timestamp <= previous
    {
        return Err(error(
            coordinate,
            ToneBuildErrorKind::NonIncreasingTimestamp {
                index,
                previous,
                current: timestamp,
            },
        ));
    }
    try_push(
        timestamps,
        timestamp,
        coordinate,
        ToneRecordBuffer::Timestamps,
    )
}

fn try_push<T>(
    values: &mut Vec<T>,
    value: T,
    coordinate: ToneCoordinate,
    buffer: ToneRecordBuffer,
) -> Result<(), ToneBuildError> {
    let requested_capacity = values
        .len()
        .checked_add(1)
        .ok_or_else(|| error(coordinate, ToneBuildErrorKind::CapacityOverflow { buffer }))?;
    values.try_reserve(1).map_err(|_| {
        error(
            coordinate,
            ToneBuildErrorKind::CapacityReservationFailed {
                buffer,
                requested_capacity,
            },
        )
    })?;
    values.push(value);
    Ok(())
}

fn error(coordinate: ToneCoordinate, kind: ToneBuildErrorKind) -> ToneBuildError {
    ToneBuildError { coordinate, kind }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rug::{Float, float::Constant};

    use super::*;
    use crate::ButterworthModes;

    const TEST_PRECISION: u32 = 256;

    fn coordinate(
        order: ToneOrder,
        frequency: ToneFrequency,
        phase: TonePhase,
        q_max: QMax,
        schedule: ScheduleKind,
    ) -> ToneCoordinate {
        ToneCoordinate {
            order,
            frequency,
            phase,
            q_max,
            schedule,
        }
    }

    fn independent_metadata(coordinate: ToneCoordinate) -> (Float, Float, Float, Float) {
        let precision = TEST_PRECISION;
        let order = coordinate.order.value();

        let mut sqrt_two = Float::with_val(precision, 2);
        sqrt_two.sqrt_mut();
        let mut root = sqrt_two;
        root -= 1;
        let mut exponent = Float::with_val(precision, -1);
        exponent /= 2 * order;
        let mut alpha_log = root.ln();
        alpha_log *= exponent;
        alpha_log.exp_mut();
        let alpha = alpha_log;

        let pi = Float::with_val(precision, Constant::Pi);
        let mut angle = Float::with_val(precision, &pi);
        angle /= 2 * order;
        let (sine, _) = angle.sin_cos(Float::new(precision));
        let mut gamma = Float::with_val(precision, &alpha);
        gamma *= sine;
        let mut guard = Float::with_val(precision, 24);
        guard /= &gamma;

        let (numerator, denominator) = coordinate.frequency.ratio();
        let mut frequency = Float::with_val(precision, numerator);
        frequency /= denominator;
        let mut period = Float::with_val(precision, pi);
        period *= 2;
        period /= frequency;
        (alpha, gamma, guard, period)
    }

    fn direct_tick_timestamp(ticks: u64, denominator: u64) -> Float {
        let mut timestamp = Float::with_val(TEST_PRECISION, Constant::Pi);
        timestamp *= ticks;
        timestamp /= denominator;
        timestamp
    }

    fn direct_tick_interval(ticks: u64, denominator: u64) -> Float {
        let mut interval = Float::with_val(TEST_PRECISION, Constant::Pi);
        interval *= ticks;
        interval /= denominator;
        interval
    }

    fn float_representation(value: &Float) -> (u32, String) {
        (value.prec(), value.to_string_radix(2, None))
    }

    fn same_record_bits(left: &NominalToneRecord, right: &NominalToneRecord) {
        assert_eq!(left.coordinate(), right.coordinate());
        assert_eq!(left.full_tick_count(), right.full_tick_count());
        assert_eq!(left.full_interval_count(), right.full_interval_count());
        assert_eq!(
            left.normalized_intervals().len(),
            right.normalized_intervals().len()
        );
        for (a, b) in left
            .normalized_intervals()
            .iter()
            .zip(right.normalized_intervals())
        {
            assert_eq!(float_representation(a), float_representation(b));
        }
        assert_eq!(
            left.normalized_times()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            right
                .normalized_times()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            left.samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            right
                .samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
        for (a, b) in [
            (left.metadata().guard(), right.metadata().guard()),
            (left.metadata().period(), right.metadata().period()),
            (
                left.metadata().nominal_endpoint(),
                right.metadata().nominal_endpoint(),
            ),
        ] {
            assert_eq!(float_representation(a), float_representation(b));
        }
    }

    #[test]
    fn axis_declaration_order_and_nominal_count_are_frozen() {
        assert_eq!(
            TONE_ORDERS,
            [
                ToneOrder::One,
                ToneOrder::Two,
                ToneOrder::Four,
                ToneOrder::Eight,
            ]
        );
        assert_eq!(
            TONE_FREQUENCIES,
            [
                ToneFrequency::OneSixteenth,
                ToneFrequency::OneQuarter,
                ToneFrequency::SevenTenths,
                ToneFrequency::One,
                ToneFrequency::Two,
            ]
        );
        assert_eq!(
            TONE_PHASES,
            [TonePhase::SeventeenHundredths, TonePhase::OneNineteen]
        );
        assert_eq!(TONE_RECORD_COUNT, 4 * 5 * 2 * 3 * 3);
        assert_eq!(TONE_RECORD_COUNT, 360);

        assert_eq!(ToneFrequency::SevenTenths.ratio(), (7, 10));
        let mut exact_seven_tenths = Float::with_val(TEST_PRECISION, 7);
        exact_seven_tenths /= 10;
        let promoted_binary_seven_tenths = Float::with_val(TEST_PRECISION, 0.7_f64);
        assert_ne!(exact_seven_tenths, promoted_binary_seven_tenths);
    }

    #[test]
    fn exhaustive_nominal_tone_records_are_finite_bounded_and_structurally_closed() {
        let mut count = 0;
        let mut largest_shape = (0_usize, 0_usize);
        let mut largest_coordinate = None;

        for order in TONE_ORDERS {
            for frequency in TONE_FREQUENCIES {
                for phase in TONE_PHASES {
                    for q_max in crate::Q_MAXES {
                        for schedule in crate::SCHEDULES {
                            let coordinate = coordinate(order, frequency, phase, q_max, schedule);
                            let record = build_nominal_tone_record(coordinate).unwrap();
                            count += 1;

                            let intervals = record.normalized_intervals();
                            let timestamps = record.normalized_times();
                            let samples = record.samples();
                            assert_eq!(intervals.len() + 1, timestamps.len());
                            assert_eq!(timestamps.len(), samples.len());
                            assert!(
                                intervals.len() == record.full_interval_count()
                                    || intervals.len()
                                        == record.full_interval_count().checked_add(1).unwrap()
                            );
                            assert_eq!(timestamps[0].to_bits(), 0.0_f64.to_bits());
                            assert_eq!(
                                timestamps.last().unwrap().to_bits(),
                                record.metadata().nominal_endpoint().to_f64().to_bits()
                            );

                            let endpoint = record.metadata().nominal_endpoint();
                            let denominator = q_max.tick_denominator();
                            let pattern = schedule.tick_pattern();
                            let mut current_ticks = 0_u64;
                            for interval_index in 0..record.full_interval_count() {
                                let delta = pattern[interval_index % pattern.len()];
                                current_ticks = current_ticks.checked_add(delta).unwrap();
                                let expected_interval = direct_tick_interval(delta, denominator);
                                let actual_interval = &intervals[interval_index];
                                assert_eq!(actual_interval.prec(), TEST_PRECISION);
                                assert_eq!(actual_interval, &expected_interval);
                                assert!(actual_interval > &0);

                                let expected_tau =
                                    direct_tick_timestamp(current_ticks, denominator);
                                assert!(&expected_tau <= endpoint);
                                let timestamp_index = interval_index + 1;
                                assert_eq!(
                                    timestamps[timestamp_index].to_bits(),
                                    expected_tau.to_f64().to_bits(),
                                    "{coordinate:?} timestamp {timestamp_index}"
                                );
                            }
                            assert_eq!(current_ticks, record.full_tick_count());

                            let q_max_value = {
                                let mut value = Float::with_val(TEST_PRECISION, Constant::Pi);
                                value /= q_max.denominator();
                                value
                            };
                            for (index, interval) in intervals.iter().enumerate() {
                                assert!(interval.is_finite() && interval > &0);
                                assert!(interval <= &q_max_value, "{coordinate:?} {index}");
                            }

                            if intervals.len() > record.full_interval_count() {
                                let last_tau = direct_tick_timestamp(current_ticks, denominator);
                                let remainder = &intervals[record.full_interval_count()];
                                assert_eq!(remainder.prec(), TEST_PRECISION);
                                let mut closure = last_tau;
                                closure += remainder;
                                assert_eq!(&closure, endpoint, "{coordinate:?} structural closure");
                                let next_ticks = current_ticks
                                    .checked_add(
                                        pattern[record.full_interval_count() % pattern.len()],
                                    )
                                    .unwrap();
                                assert!(direct_tick_timestamp(next_ticks, denominator) > *endpoint);
                            } else {
                                assert_eq!(intervals.len(), record.full_interval_count());
                                assert_eq!(
                                    &direct_tick_timestamp(current_ticks, denominator),
                                    endpoint
                                );
                            }

                            for (index, (&timestamp, &sample)) in
                                timestamps.iter().zip(samples).enumerate()
                            {
                                assert!(timestamp.is_finite());
                                if index > 0 {
                                    assert!(timestamp > timestamps[index - 1]);
                                }
                                assert!(sample.is_finite());
                                assert!(sample.abs() <= 1.0 + 4.0 * f64::EPSILON);
                                let independent_frequency = {
                                    let (numerator, denominator) = frequency.ratio();
                                    f64::from(numerator) / f64::from(denominator)
                                };
                                let independent_sample =
                                    (independent_frequency * timestamp + phase.radians()).cos();
                                assert_eq!(sample.to_bits(), independent_sample.to_bits());
                            }

                            assert_metadata_relations(&record);
                            if samples.len() > largest_shape.1 {
                                largest_shape = (intervals.len(), samples.len());
                                largest_coordinate = Some(coordinate);
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(count, TONE_RECORD_COUNT);
        assert_eq!(largest_shape, (33_013, 33_014));
        assert_eq!(
            largest_coordinate,
            Some(coordinate(
                ToneOrder::Eight,
                ToneFrequency::OneSixteenth,
                TonePhase::SeventeenHundredths,
                QMax::PiOver128,
                ScheduleKind::Uneven,
            ))
        );
    }

    fn assert_metadata_relations(record: &NominalToneRecord) {
        let coordinate = record.coordinate();
        for value in [
            record.metadata().guard(),
            record.metadata().period(),
            record.metadata().nominal_endpoint(),
        ] {
            assert_eq!(value.prec(), TEST_PRECISION);
            assert!(value.is_finite() && value > &0);
        }

        let (alpha, gamma, expected_guard, expected_period) = independent_metadata(coordinate);
        let modes = ButterworthModes::new(coordinate.order.value(), TEST_PRECISION);
        assert_eq!(&alpha, modes.alpha());
        let expected_guard_f64 = expected_guard.to_f64();
        assert_eq!(record.metadata().guard(), &expected_guard);
        assert_eq!(
            record.metadata().guard().to_f64().to_bits(),
            expected_guard_f64.to_bits()
        );
        assert!(gamma > 0);
        assert_eq!(record.metadata().period(), &expected_period);
        assert_eq!(
            record.metadata().period().to_f64().to_bits(),
            expected_period.to_f64().to_bits()
        );

        let mut endpoint = record.metadata().guard().clone();
        endpoint *= 2;
        endpoint += record.metadata().period();
        assert_eq!(&endpoint, record.metadata().nominal_endpoint());
    }

    #[test]
    fn shared_timestamps_for_same_frequency_and_phase_have_identical_sample_bits() {
        let mut seen = BTreeMap::<u64, u64>::new();
        for order in TONE_ORDERS {
            for schedule in crate::SCHEDULES {
                let record = build_nominal_tone_record(coordinate(
                    order,
                    ToneFrequency::SevenTenths,
                    TonePhase::OneNineteen,
                    QMax::PiOver32,
                    schedule,
                ))
                .unwrap();
                for (&timestamp, &sample) in record.normalized_times().iter().zip(record.samples())
                {
                    if let Some(previous) = seen.insert(timestamp.to_bits(), sample.to_bits()) {
                        assert_eq!(previous, sample.to_bits());
                    }
                }
            }
        }
        assert!(seen.len() > 100);
    }

    #[test]
    fn deterministic_records_match_every_frozen_float_and_scalar_bit() {
        let selected = coordinate(
            ToneOrder::Eight,
            ToneFrequency::SevenTenths,
            TonePhase::OneNineteen,
            QMax::PiOver128,
            ScheduleKind::Uneven,
        );
        let first = build_nominal_tone_record(selected).unwrap();
        let second = build_nominal_tone_record(selected).unwrap();
        same_record_bits(&first, &second);
    }

    #[test]
    fn nominal_formula_sentinels_freeze_order_and_exact_frequency_ratios() {
        let cases = [
            (ToneOrder::One, ToneFrequency::OneSixteenth),
            (ToneOrder::One, ToneFrequency::SevenTenths),
            (ToneOrder::One, ToneFrequency::Two),
            (ToneOrder::Eight, ToneFrequency::OneSixteenth),
            (ToneOrder::Eight, ToneFrequency::SevenTenths),
            (ToneOrder::Eight, ToneFrequency::Two),
        ];
        let mut observed = Vec::new();
        for (order, frequency) in cases {
            let record = build_nominal_tone_record(coordinate(
                order,
                frequency,
                TonePhase::SeventeenHundredths,
                QMax::PiOver32,
                ScheduleKind::Uniform,
            ))
            .unwrap();
            observed.push((
                record.metadata().guard().to_f64().to_bits(),
                record.metadata().period().to_f64().to_bits(),
                record.metadata().nominal_endpoint().to_f64().to_bits(),
                record.samples()[1].to_bits(),
            ));
        }
        assert_eq!(
            observed,
            [
                (
                    4_624_885_090_567_448_524,
                    4_636_774_654_688_898_328,
                    4_638_828_069_504_340_870,
                    4_607_043_060_585_671_954,
                ),
                (
                    4_624_885_090_567_448_524,
                    4_621_242_645_020_493_000,
                    4_630_807_810_307_278_104,
                    4_606_926_982_602_865_987,
                ),
                (
                    4_624_885_090_567_448_524,
                    4_614_256_656_552_045_848,
                    4_629_986_693_425_346_232,
                    4_606_584_711_803_202_206,
                ),
                (
                    4_637_893_207_187_921_475,
                    4_636_774_654_688_898_328,
                    4_644_572_570_755_143_784,
                    4_607_043_060_585_671_954,
                ),
                (
                    4_637_893_207_187_921_475,
                    4_621_242_645_020_493_000,
                    4_642_712_621_000_650_384,
                    4_606_926_982_602_865_987,
                ),
                (
                    4_637_893_207_187_921_475,
                    4_614_256_656_552_045_848,
                    4_642_507_341_780_167_416,
                    4_606_584_711_803_202_206,
                ),
            ]
        );
    }

    #[test]
    fn known_non_tick_endpoint_has_one_frozen_positive_remainder() {
        let record = build_nominal_tone_record(coordinate(
            ToneOrder::One,
            ToneFrequency::OneSixteenth,
            TonePhase::SeventeenHundredths,
            QMax::PiOver128,
            ScheduleKind::Uniform,
        ))
        .unwrap();
        assert_eq!(record.full_tick_count(), 171_328);
        assert_eq!(record.full_interval_count(), 5_354);
        assert_eq!(record.normalized_intervals().len(), 5_355);
        assert_eq!(
            record
                .normalized_intervals()
                .last()
                .unwrap()
                .to_f64()
                .to_bits(),
            4_580_429_982_351_333_059
        );
        assert_eq!(
            record.metadata().nominal_endpoint().to_f64().to_bits(),
            4_638_828_069_504_340_870
        );
    }
}
