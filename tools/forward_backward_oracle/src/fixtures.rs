use core::fmt;

use rug::{Float, float::Constant};

const FIXTURE_PRECISION: u32 = 256;
const PI: f64 = core::f64::consts::PI;
const RECORD_END: f64 = 128.0 * PI;

/// The seven frozen analytic records used by the forward-backward matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeDomainFixture {
    UnequalMultitone,
    GaussianPulse,
    TaperedChirp,
    InteriorStep,
    LeftAdjacentStep,
    RightAdjacentStep,
    TriangularPulse,
}

/// Stable declaration order for the shared-duration time-domain records.
pub const TIME_DOMAIN_FIXTURES: [TimeDomainFixture; 7] = [
    TimeDomainFixture::UnequalMultitone,
    TimeDomainFixture::GaussianPulse,
    TimeDomainFixture::TaperedChirp,
    TimeDomainFixture::InteriorStep,
    TimeDomainFixture::LeftAdjacentStep,
    TimeDomainFixture::RightAdjacentStep,
    TimeDomainFixture::TriangularPulse,
];

impl TimeDomainFixture {
    /// The analytic absolute-amplitude bound from the fixture specification.
    #[must_use]
    pub const fn amplitude_bound(self) -> f64 {
        match self {
            Self::UnequalMultitone => 2.52,
            Self::InteriorStep | Self::LeftAdjacentStep | Self::RightAdjacentStep => 0.5,
            Self::GaussianPulse | Self::TaperedChirp | Self::TriangularPulse => 1.0,
        }
    }
}

/// A maximum normalized interval `pi / denominator`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QMax {
    PiOver128,
    PiOver64,
    PiOver32,
}

/// Stable declaration order for all frozen maximum intervals.
pub const Q_MAXES: [QMax; 3] = [QMax::PiOver128, QMax::PiOver64, QMax::PiOver32];

impl QMax {
    /// The denominator in `q_max = pi / denominator`.
    #[must_use]
    pub const fn denominator(self) -> u64 {
        match self {
            Self::PiOver128 => 128,
            Self::PiOver64 => 64,
            Self::PiOver32 => 32,
        }
    }

    /// The exact integer-tick denominator for one normalized timestamp.
    ///
    /// One tick is `q_max / 32 = pi / tick_denominator`.
    #[must_use]
    pub const fn tick_denominator(self) -> u64 {
        self.denominator() * 32
    }

    /// The exact final tick count for the shared endpoint `128*pi`.
    #[must_use]
    pub const fn total_ticks(self) -> u64 {
        match self {
            Self::PiOver128 => 524_288,
            Self::PiOver64 => 262_144,
            Self::PiOver32 => 131_072,
        }
    }
}

/// A frozen normalized interval schedule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleKind {
    Uniform,
    Jitter,
    Uneven,
}

/// Stable declaration order for all frozen schedules.
pub const SCHEDULES: [ScheduleKind; 3] = [
    ScheduleKind::Uniform,
    ScheduleKind::Jitter,
    ScheduleKind::Uneven,
];

impl ScheduleKind {
    /// The exact tick increments in one repeating schedule pattern.
    #[must_use]
    pub const fn tick_pattern(self) -> &'static [u64] {
        match self {
            Self::Uniform => &[32],
            Self::Jitter => &[16, 32, 24, 8, 28, 20],
            Self::Uneven => &[32, 1, 16, 2, 24, 4],
        }
    }
}

/// The coordinate identifying one nominal fixture record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixtureCoordinate {
    pub fixture: TimeDomainFixture,
    pub q_max: QMax,
    pub schedule: ScheduleKind,
}

/// Contextual failure while constructing one exact-tick fixture record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FixtureBuildError {
    TickCountOverflow {
        coordinate: FixtureCoordinate,
        interval_index: usize,
    },
    NonFiniteTimestamp {
        coordinate: FixtureCoordinate,
        index: usize,
    },
    NonIncreasingTimestamp {
        coordinate: FixtureCoordinate,
        index: usize,
        previous: f64,
        current: f64,
    },
    NonFiniteSample {
        coordinate: FixtureCoordinate,
        index: usize,
        value: f64,
    },
}

impl fmt::Display for FixtureBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TickCountOverflow {
                coordinate,
                interval_index,
            } => write!(
                formatter,
                "tick count overflow at interval {interval_index} for {coordinate:?}"
            ),
            Self::NonFiniteTimestamp { coordinate, index } => {
                write!(
                    formatter,
                    "timestamp {index} is not finite for {coordinate:?}"
                )
            }
            Self::NonIncreasingTimestamp {
                coordinate,
                index,
                previous,
                current,
            } => write!(
                formatter,
                "timestamp {index} ({current}) is not greater than its predecessor ({previous}) for {coordinate:?}"
            ),
            Self::NonFiniteSample {
                coordinate,
                index,
                value,
            } => write!(
                formatter,
                "sample {index} ({value}) is not finite for {coordinate:?}"
            ),
        }
    }
}

impl std::error::Error for FixtureBuildError {}

/// One fully constructed analytic fixture record.
///
/// The integer tick positions preserve the nominal schedule exactly. Each
/// normalized timestamp is independently computed at 256-bit precision and
/// rounded to `f64` once, then each analytic sample is evaluated from that
/// timestamp.
#[derive(Debug, PartialEq)]
pub struct NominalRecord {
    coordinate: FixtureCoordinate,
    tick_positions: Vec<u64>,
    normalized_times: Vec<f64>,
    samples: Vec<f64>,
}

impl NominalRecord {
    #[must_use]
    pub const fn coordinate(&self) -> FixtureCoordinate {
        self.coordinate
    }

    /// Exact integer tick positions, including the initial zero and final tick.
    #[must_use]
    pub fn tick_positions(&self) -> &[u64] {
        &self.tick_positions
    }

    /// Independently rounded normalized timestamps corresponding to the ticks.
    #[must_use]
    pub fn normalized_times(&self) -> &[f64] {
        &self.normalized_times
    }

    /// Analytic samples evaluated at `normalized_times()`.
    #[must_use]
    pub fn samples(&self) -> &[f64] {
        &self.samples
    }
}

/// Build one of the seven time-domain fixture records.
///
/// Only the requested record is materialized. Schedule generation accumulates
/// integer ticks, and all timestamps are calculated independently from those
/// ticks rather than by floating-point interval addition.
pub fn build_nominal_record(
    coordinate: FixtureCoordinate,
) -> Result<NominalRecord, FixtureBuildError> {
    let tick_positions = build_tick_positions(coordinate)?;
    let tick_denominator = coordinate.q_max.tick_denominator();
    let pi = Float::with_val(FIXTURE_PRECISION, Constant::Pi);
    let mut normalized_times = Vec::with_capacity(tick_positions.len());

    for (index, tick) in tick_positions.iter().copied().enumerate() {
        let mut timestamp = pi.clone();
        timestamp *= tick;
        timestamp /= tick_denominator;
        let timestamp = timestamp.to_f64();
        if !timestamp.is_finite() {
            return Err(FixtureBuildError::NonFiniteTimestamp { coordinate, index });
        }
        if let Some(previous) = normalized_times.last().copied()
            && timestamp <= previous
        {
            return Err(FixtureBuildError::NonIncreasingTimestamp {
                coordinate,
                index,
                previous,
                current: timestamp,
            });
        }
        normalized_times.push(timestamp);
    }

    let mut samples = Vec::with_capacity(normalized_times.len());
    for (index, timestamp) in normalized_times.iter().copied().enumerate() {
        let value = analytic_fixture_value(coordinate.fixture, timestamp);
        if !value.is_finite() {
            return Err(FixtureBuildError::NonFiniteSample {
                coordinate,
                index,
                value,
            });
        }
        samples.push(value);
    }

    Ok(NominalRecord {
        coordinate,
        tick_positions,
        normalized_times,
        samples,
    })
}

fn build_tick_positions(coordinate: FixtureCoordinate) -> Result<Vec<u64>, FixtureBuildError> {
    let total_ticks = coordinate.q_max.total_ticks();
    let pattern = coordinate.schedule.tick_pattern();
    let mut positions = Vec::with_capacity((total_ticks / 32 + 1) as usize);
    positions.push(0);

    let mut current = 0_u64;
    let mut interval_index = 0_usize;
    while current < total_ticks {
        let remaining = total_ticks - current;
        let increment = pattern[interval_index % pattern.len()].min(remaining);
        if increment == 0 {
            return Err(FixtureBuildError::TickCountOverflow {
                coordinate,
                interval_index,
            });
        }
        current = current
            .checked_add(increment)
            .ok_or(FixtureBuildError::TickCountOverflow {
                coordinate,
                interval_index,
            })?;
        positions.push(current);
        interval_index += 1;
    }

    Ok(positions)
}

/// Evaluate a frozen time-domain formula at one normalized timestamp.
///
/// This function takes no schedule or matrix coordinate, so a shared `tau`
/// always has the same analytic value.
#[must_use]
pub fn analytic_fixture_value(fixture: TimeDomainFixture, tau: f64) -> f64 {
    match fixture {
        TimeDomainFixture::UnequalMultitone => {
            const RATIOS: [f64; 5] = [
                5.0 / 64.0,
                13.0 / 64.0,
                25.0 / 64.0,
                37.0 / 64.0,
                43.0 / 64.0,
            ];
            const AMPLITUDES: [f64; 5] = [1.0, 0.63, 0.41, 0.29, 0.19];
            const PHASES: [f64; 5] = [0.17, -0.73, 1.19, -1.41, 0.48];
            let mut value = 0.0;
            for index in 0..RATIOS.len() {
                value += AMPLITUDES[index] * (RATIOS[index] * tau + PHASES[index]).cos();
            }
            value
        }
        TimeDomainFixture::GaussianPulse => {
            let center = RECORD_END / 2.0;
            let sigma = 3.0 * PI;
            let displacement = tau - center;
            let envelope = (-0.5 * (displacement / sigma).powi(2)).exp();
            envelope * (0.375 * displacement + 0.37).cos()
        }
        TimeDomainFixture::TaperedChirp => chirp_value(tau),
        TimeDomainFixture::InteriorStep => step_value(RECORD_END / 2.0, tau),
        TimeDomainFixture::LeftAdjacentStep => step_value(4.0 * PI, tau),
        TimeDomainFixture::RightAdjacentStep => step_value(4.0 * PI, RECORD_END - tau),
        TimeDomainFixture::TriangularPulse => {
            let center = RECORD_END / 2.0;
            let half_width = 4.0 * PI;
            (1.0 - (tau - center).abs() / half_width).max(0.0)
        }
    }
}

fn step_value(center: f64, tau: f64) -> f64 {
    if tau <= center - PI / 2.0 {
        -0.5
    } else if tau >= center + PI / 2.0 {
        0.5
    } else {
        (tau - center) / PI
    }
}

fn chirp_phase(tau: f64) -> f64 {
    let start_frequency = 0.05;
    let end_frequency = 0.65;
    0.23 + start_frequency * tau
        + (end_frequency - start_frequency) * tau * tau / (2.0 * RECORD_END)
}

fn chirp_window(tau: f64) -> f64 {
    if tau == 0.0 || tau == RECORD_END {
        return 0.0;
    }
    let sine = (PI * tau / RECORD_END).sin();
    sine * sine
}

#[cfg(test)]
fn chirp_window_derivative(tau: f64) -> f64 {
    if tau == 0.0 || tau == RECORD_END {
        return 0.0;
    }
    (PI / RECORD_END) * (2.0 * PI * tau / RECORD_END).sin()
}

fn chirp_value(tau: f64) -> f64 {
    if tau == 0.0 || tau == RECORD_END {
        return 0.0;
    }
    chirp_window(tau) * chirp_phase(tau).cos()
}

#[cfg(test)]
fn chirp_derivative(tau: f64) -> f64 {
    if tau == 0.0 || tau == RECORD_END {
        return 0.0;
    }
    let frequency = 0.05 + 0.60 * tau / RECORD_END;
    chirp_window_derivative(tau) * chirp_phase(tau).cos()
        - chirp_window(tau) * chirp_phase(tau).sin() * frequency
}

#[cfg(test)]
mod tests {
    use rug::{Float, float::Constant};

    use super::*;
    use crate::{RepresentedRecord, evaluate};

    const REFERENCE_PRECISION: u32 = 256;

    fn coordinate(
        fixture: TimeDomainFixture,
        q_max: QMax,
        schedule: ScheduleKind,
    ) -> FixtureCoordinate {
        FixtureCoordinate {
            fixture,
            q_max,
            schedule,
        }
    }

    fn direct_timestamp(tick: u64, q_max: QMax) -> f64 {
        let mut timestamp = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        timestamp *= tick;
        timestamp /= q_max.tick_denominator();
        timestamp.to_f64()
    }

    fn independently_rounded_record_end() -> f64 {
        let mut endpoint = Float::with_val(REFERENCE_PRECISION, Constant::Pi);
        endpoint *= 128;
        endpoint.to_f64()
    }

    fn assert_shared_samples(left: &NominalRecord, right: &NominalRecord) -> usize {
        let left_denominator = u128::from(left.coordinate().q_max.tick_denominator());
        let right_denominator = u128::from(right.coordinate().q_max.tick_denominator());
        let mut left_index = 0;
        let mut right_index = 0;
        let mut shared_interior = 0;

        while left_index < left.tick_positions().len() && right_index < right.tick_positions().len()
        {
            let left_fraction = u128::from(left.tick_positions()[left_index]) * right_denominator;
            let right_fraction = u128::from(right.tick_positions()[right_index]) * left_denominator;
            match left_fraction.cmp(&right_fraction) {
                core::cmp::Ordering::Less => left_index += 1,
                core::cmp::Ordering::Greater => right_index += 1,
                core::cmp::Ordering::Equal => {
                    let left_tau = left.normalized_times()[left_index];
                    let right_tau = right.normalized_times()[right_index];
                    assert_eq!(left_tau.to_bits(), right_tau.to_bits());
                    assert_eq!(
                        left.samples()[left_index].to_bits(),
                        right.samples()[right_index].to_bits()
                    );
                    if left_index != 0
                        && left_index + 1 < left.tick_positions().len()
                        && right_index != 0
                        && right_index + 1 < right.tick_positions().len()
                    {
                        shared_interior += 1;
                    }
                    left_index += 1;
                    right_index += 1;
                }
            }
        }

        shared_interior
    }

    #[test]
    fn declaration_arrays_freeze_the_required_coordinate_order() {
        assert_eq!(
            TIME_DOMAIN_FIXTURES,
            [
                TimeDomainFixture::UnequalMultitone,
                TimeDomainFixture::GaussianPulse,
                TimeDomainFixture::TaperedChirp,
                TimeDomainFixture::InteriorStep,
                TimeDomainFixture::LeftAdjacentStep,
                TimeDomainFixture::RightAdjacentStep,
                TimeDomainFixture::TriangularPulse,
            ]
        );
        assert_eq!(Q_MAXES, [QMax::PiOver128, QMax::PiOver64, QMax::PiOver32]);
        assert_eq!(
            SCHEDULES,
            [
                ScheduleKind::Uniform,
                ScheduleKind::Jitter,
                ScheduleKind::Uneven,
            ]
        );
    }

    #[test]
    fn every_schedule_has_exact_tick_closure_and_frozen_pattern_semantics() {
        for q_max in Q_MAXES {
            for schedule in SCHEDULES {
                let coord = coordinate(TimeDomainFixture::UnequalMultitone, q_max, schedule);
                let record = build_nominal_record(coord).unwrap();
                let ticks = record.tick_positions();
                let times = record.normalized_times();
                assert_eq!(ticks[0], 0);
                assert_eq!(*ticks.last().unwrap(), q_max.total_ticks());
                assert_eq!(times[0], 0.0);
                assert_eq!(*times.last().unwrap(), independently_rounded_record_end());
                assert_eq!(ticks.len(), times.len());

                let pattern = schedule.tick_pattern();
                for index in 0..ticks.len() - 1 {
                    let delta = ticks[index + 1] - ticks[index];
                    let remaining = q_max.total_ticks() - ticks[index];
                    assert!(delta > 0, "{coord:?} interval {index} has zero ticks");
                    assert!(delta <= 32, "{coord:?} interval {index} exceeds q_max");
                    assert_eq!(
                        delta,
                        pattern[index % pattern.len()].min(remaining),
                        "{coord:?} interval {index} does not follow its reset pattern"
                    );
                    assert!(times[index + 1] > times[index]);
                }

                for (index, (&tick, &time)) in ticks.iter().zip(times).enumerate() {
                    assert_eq!(
                        time,
                        direct_timestamp(tick, q_max),
                        "{coord:?} timestamp {index}"
                    );
                }

                let original_ticks = ticks.to_vec();
                drop(record);
                let second = build_nominal_record(coordinate(
                    TimeDomainFixture::TriangularPulse,
                    q_max,
                    schedule,
                ))
                .unwrap();
                assert_eq!(second.tick_positions(), original_ticks);
            }
        }
    }

    #[test]
    fn all_seven_by_nine_records_are_finite_bounded_and_nontrivial() {
        let allowance_factor = 64.0 * f64::EPSILON;
        for fixture in TIME_DOMAIN_FIXTURES {
            let bound = fixture.amplitude_bound();
            let allowance = allowance_factor * bound.max(1.0);
            for q_max in Q_MAXES {
                for schedule in SCHEDULES {
                    let record =
                        build_nominal_record(coordinate(fixture, q_max, schedule)).unwrap();
                    let mut maximum = 0.0_f64;
                    let mut energy = 0.0_f64;
                    for (index, &sample) in record.samples().iter().enumerate() {
                        assert!(sample.is_finite(), "{fixture:?} sample {index}");
                        assert!(
                            sample.abs() <= bound + allowance,
                            "{fixture:?} sample {index} has magnitude {} beyond {} + {}",
                            sample.abs(),
                            bound,
                            allowance
                        );
                        maximum = maximum.max(sample.abs());
                        energy += sample * sample;
                    }
                    assert!(maximum > 0.0, "{fixture:?} has zero sampled scale");
                    assert!(
                        energy.is_finite() && energy > 0.0,
                        "{fixture:?} has no energy"
                    );
                }
            }
        }
    }

    #[test]
    fn construction_is_deterministic() {
        let selected = coordinate(
            TimeDomainFixture::GaussianPulse,
            QMax::PiOver64,
            ScheduleKind::Jitter,
        );
        let first = build_nominal_record(selected).unwrap();
        let second = build_nominal_record(selected).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn shared_ticks_across_schedules_and_qmax_grids_have_identical_sample_bits() {
        let uniform = build_nominal_record(coordinate(
            TimeDomainFixture::TaperedChirp,
            QMax::PiOver32,
            ScheduleKind::Uniform,
        ))
        .unwrap();
        let uneven = build_nominal_record(coordinate(
            TimeDomainFixture::TaperedChirp,
            QMax::PiOver32,
            ScheduleKind::Uneven,
        ))
        .unwrap();
        assert!(assert_shared_samples(&uniform, &uneven) > 0);

        let fine_grid = build_nominal_record(coordinate(
            TimeDomainFixture::GaussianPulse,
            QMax::PiOver64,
            ScheduleKind::Uniform,
        ))
        .unwrap();
        let coarse_grid = build_nominal_record(coordinate(
            TimeDomainFixture::GaussianPulse,
            QMax::PiOver32,
            ScheduleKind::Uniform,
        ))
        .unwrap();
        assert!(assert_shared_samples(&coarse_grid, &fine_grid) > 0);
    }

    #[test]
    fn independent_multitone_dft_recovers_every_frozen_component() {
        const HARMONICS: [f64; 5] = [5.0, 13.0, 25.0, 37.0, 43.0];
        const AMPLITUDES: [f64; 5] = [1.0, 0.63, 0.41, 0.29, 0.19];
        const PHASES: [f64; 5] = [0.17, -0.73, 1.19, -1.41, 0.48];
        const GRID_SIZE: usize = 128;
        const T: f64 = 128.0 * core::f64::consts::PI;

        let mut cosine_coefficients = [0.0; 5];
        let mut sine_coefficients = [0.0; 5];
        for index in 0..GRID_SIZE {
            let cycle_angle = 2.0 * core::f64::consts::PI * index as f64 / GRID_SIZE as f64;
            let tau = T * index as f64 / GRID_SIZE as f64;
            let sample = analytic_fixture_value(TimeDomainFixture::UnequalMultitone, tau);
            for component in 0..HARMONICS.len() {
                let angle = HARMONICS[component] * cycle_angle;
                cosine_coefficients[component] += sample * angle.cos();
                sine_coefficients[component] += sample * angle.sin();
            }
        }

        for component in 0..HARMONICS.len() {
            let actual_cosine = 2.0 * cosine_coefficients[component] / GRID_SIZE as f64;
            let actual_sine = 2.0 * sine_coefficients[component] / GRID_SIZE as f64;
            let expected_cosine = AMPLITUDES[component] * PHASES[component].cos();
            let expected_sine = -AMPLITUDES[component] * PHASES[component].sin();
            // 128 trigonometric evaluations and their accumulation contribute
            // only a few dozen f64 ulps to these unit-scale Fourier moments.
            assert!((actual_cosine - expected_cosine).abs() <= 5.0e-13);
            assert!((actual_sine - expected_sine).abs() <= 5.0e-13);
        }
    }

    #[test]
    fn independent_formula_checkpoints_freeze_gaussian_chirp_and_triangle_constants() {
        const PI: f64 = core::f64::consts::PI;
        const T: f64 = 128.0 * PI;
        let gaussian_center = T / 2.0;
        for displacement in [0.0, 3.0 * PI, -6.0 * PI] {
            let tau = gaussian_center + displacement;
            let expected = (-0.5 * (displacement / (3.0 * PI)).powi(2)).exp()
                * (0.375 * displacement + 0.37).cos();
            let actual = analytic_fixture_value(TimeDomainFixture::GaussianPulse, tau);
            // Leave a small margin for platform libm differences in exp/cos.
            assert!((actual - expected).abs() <= 2.0e-14);
        }

        for tau in [T / 4.0, T / 2.0, 3.0 * T / 4.0] {
            let independent_window = (PI * tau / T).sin().powi(2);
            let independent_phase = 0.23 + 0.05 * tau + (0.65 - 0.05) * tau * tau / (2.0 * T);
            let expected = independent_window * independent_phase.cos();
            let actual = analytic_fixture_value(TimeDomainFixture::TaperedChirp, tau);
            // The independent f64 expression uses the same analytic constants
            // but may round its argument differently before libm evaluation.
            assert!((actual - expected).abs() <= 2.0e-14);
        }

        let triangle_center = T / 2.0;
        for displacement in [0.0, PI, -PI, 2.0 * PI, -2.0 * PI] {
            let expected = 1.0 - displacement.abs() / (4.0 * PI);
            let actual = analytic_fixture_value(
                TimeDomainFixture::TriangularPulse,
                triangle_center + displacement,
            );
            assert!((actual - expected).abs() <= 4.0 * f64::EPSILON);
        }
        assert!(
            (analytic_fixture_value(
                TimeDomainFixture::TriangularPulse,
                triangle_center + 2.0 * PI
            ) - 0.5)
                .abs()
                <= 4.0 * f64::EPSILON
        );
        assert!(
            (analytic_fixture_value(
                TimeDomainFixture::TriangularPulse,
                triangle_center - 2.0 * PI
            ) - 0.5)
                .abs()
                <= 4.0 * f64::EPSILON
        );
    }

    #[test]
    fn fixtures_have_their_frozen_center_endpoint_and_closure_values() {
        let center = RECORD_END / 2.0;
        let multitone_start = analytic_fixture_value(TimeDomainFixture::UnequalMultitone, 0.0);
        let multitone_end = analytic_fixture_value(TimeDomainFixture::UnequalMultitone, RECORD_END);
        assert!((multitone_start - multitone_end).abs() <= 1.0e-12);

        let gaussian_center = analytic_fixture_value(TimeDomainFixture::GaussianPulse, center);
        assert!((gaussian_center - 0.37_f64.cos()).abs() <= 8.0 * f64::EPSILON);
        let gaussian_endpoint_envelope = (-(2048.0 / 9.0_f64)).exp();
        for tau in [0.0, RECORD_END] {
            assert!(
                analytic_fixture_value(TimeDomainFixture::GaussianPulse, tau).abs()
                    <= gaussian_endpoint_envelope * (1.0 + 1.0e-12)
            );
        }

        assert_eq!(chirp_value(0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(chirp_value(RECORD_END).to_bits(), 0.0_f64.to_bits());
        assert_eq!(chirp_window_derivative(0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(
            chirp_window_derivative(RECORD_END).to_bits(),
            0.0_f64.to_bits()
        );
        assert_eq!(chirp_derivative(0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(chirp_derivative(RECORD_END).to_bits(), 0.0_f64.to_bits());
        assert!((chirp_window(center) - 1.0).abs() <= f64::EPSILON);
        assert_eq!(chirp_value(center), chirp_phase(center).cos());

        for (fixture, expected_start, expected_end) in [
            (TimeDomainFixture::InteriorStep, -0.5, 0.5),
            (TimeDomainFixture::LeftAdjacentStep, -0.5, 0.5),
            (TimeDomainFixture::RightAdjacentStep, 0.5, -0.5),
        ] {
            assert_eq!(analytic_fixture_value(fixture, 0.0), expected_start);
            assert_eq!(analytic_fixture_value(fixture, RECORD_END), expected_end);
        }

        assert_eq!(
            analytic_fixture_value(TimeDomainFixture::TriangularPulse, center),
            1.0
        );
        assert_eq!(
            analytic_fixture_value(TimeDomainFixture::TriangularPulse, 0.0),
            0.0
        );
        assert_eq!(
            analytic_fixture_value(TimeDomainFixture::TriangularPulse, RECORD_END),
            0.0
        );
        let half_width = 4.0 * PI;
        assert_eq!(
            analytic_fixture_value(TimeDomainFixture::TriangularPulse, center - half_width),
            0.0
        );
        assert_eq!(
            analytic_fixture_value(TimeDomainFixture::TriangularPulse, center + half_width),
            0.0
        );
    }

    #[test]
    fn step_plateaus_and_ramp_slopes_are_finite_and_orient_correctly() {
        let centers = [RECORD_END / 2.0, 4.0 * PI];
        for center in centers {
            assert_eq!(step_value(center, center - 2.0 * PI), -0.5);
            assert_eq!(step_value(center, center + 2.0 * PI), 0.5);

            let before = step_value(center, center - PI / 4.0);
            let after = step_value(center, center + PI / 4.0);
            let slope = (after - before) / (PI / 2.0);
            assert!((slope - 1.0 / PI).abs() <= 8.0 * f64::EPSILON);
        }

        for index in 0..=128 {
            let tau = RECORD_END * f64::from(index) / 128.0;
            let left = analytic_fixture_value(TimeDomainFixture::LeftAdjacentStep, tau);
            let reversed_right =
                analytic_fixture_value(TimeDomainFixture::RightAdjacentStep, RECORD_END - tau);
            assert!((left - reversed_right).abs() <= 2.0e-14);
        }

        let center = 4.0 * PI;
        let reversed_center = RECORD_END - center;
        let right_before = analytic_fixture_value(
            TimeDomainFixture::RightAdjacentStep,
            reversed_center - PI / 4.0,
        );
        let right_after = analytic_fixture_value(
            TimeDomainFixture::RightAdjacentStep,
            reversed_center + PI / 4.0,
        );
        let reversed_slope = (right_after - right_before) / (PI / 2.0);
        assert!((reversed_slope + 1.0 / PI).abs() <= 2.0e-14);
    }

    #[test]
    fn step_fixtures_match_independent_width_center_and_orientation_checkpoints() {
        const PI: f64 = core::f64::consts::PI;
        const T: f64 = 128.0 * PI;
        let cases = [
            (TimeDomainFixture::InteriorStep, T / 2.0, false),
            (TimeDomainFixture::LeftAdjacentStep, 4.0 * PI, false),
            (TimeDomainFixture::RightAdjacentStep, 4.0 * PI, true),
        ];

        for (fixture, center, reversed) in cases {
            for displacement in [
                -2.0 * PI,
                -PI / 2.0,
                -PI / 4.0,
                0.0,
                PI / 4.0,
                PI / 2.0,
                2.0 * PI,
            ] {
                let step_time = center + displacement;
                let tau = if reversed { T - step_time } else { step_time };
                let expected = if step_time <= center - PI / 2.0 {
                    -0.5
                } else if step_time >= center + PI / 2.0 {
                    0.5
                } else {
                    (step_time - center) / PI
                };
                let actual = analytic_fixture_value(fixture, tau);
                // Reversing around a large endpoint incurs a few ulps of
                // cancellation before the finite-slope ramp is evaluated.
                assert!((actual - expected).abs() <= 3.0e-14);
            }
        }
    }

    #[test]
    fn every_fixture_has_finite_nonzero_finite_record_oracle_output_energy() {
        for fixture in TIME_DOMAIN_FIXTURES {
            let nominal =
                build_nominal_record(coordinate(fixture, QMax::PiOver32, ScheduleKind::Uniform))
                    .unwrap();

            // This deliberately simple test-only conversion creates a valid
            // represented record; it is not the frozen scalar conversion path.
            let intervals_seconds = nominal
                .normalized_times()
                .windows(2)
                .map(|times| (times[1] - times[0]) / (2.0 * core::f64::consts::PI))
                .collect();
            let represented =
                RepresentedRecord::from_f64(1.0, intervals_seconds, nominal.samples().to_vec())
                    .unwrap();
            let oracle = evaluate(1, &represented, 192);
            assert_eq!(oracle.values().len(), nominal.samples().len());

            let mut energy = 0.0_f64;
            for (index, value) in oracle.values().iter().enumerate() {
                let converted = value.to_f64();
                assert!(converted.is_finite(), "{fixture:?} oracle value {index}");
                energy += converted * converted;
            }
            assert!(
                energy.is_finite() && energy > 0.0,
                "{fixture:?} oracle energy"
            );
        }
    }
}
