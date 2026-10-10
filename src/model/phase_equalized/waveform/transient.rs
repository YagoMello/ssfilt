//! Reusable exact-tick schedules and coupled-streaming transient scorer.

#![allow(clippy::cast_precision_loss)]

use super::spectral::Spectrum;
use super::{CoupledState, EqualizedButterworth, SampleErrors, ShapeMetrics, WeightedErrors};
use crate::model::Butterworth;
use crate::scalar::from_f64;
use crate::streaming::StreamingCore;
use crate::{InputModel, IntegrationConfig, Scalar};

pub(super) const TICKS_PER_CUTOFF_CYCLE: usize = 1_920;
pub(super) const FRAME_CYCLES: usize = 20;
pub(super) const FRAME_TICKS: usize = TICKS_PER_CUTOFF_CYCLE * FRAME_CYCLES;
const TOTAL_TICKS: usize = 2 * FRAME_TICKS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Schedule {
    Uniform,
    Jitter,
    BoundedGaps,
}

impl Schedule {
    pub(super) const ALL: [Self; 3] = [Self::Uniform, Self::Jitter, Self::BoundedGaps];

    fn interval_ticks(self, index: usize) -> usize {
        match self {
            Self::Uniform => 30,
            Self::Jitter => [18, 30, 21, 27][index % 4],
            Self::BoundedGaps => [10, 30, 20, 20][index % 4],
        }
    }

    fn boundaries(self, end_tick: usize) -> Vec<usize> {
        let mut ticks = vec![0];
        let mut interval_index = 0;
        while *ticks.last().unwrap() < end_tick {
            let next = *ticks.last().unwrap() + self.interval_ticks(interval_index);
            assert!(
                next <= end_tick,
                "schedule does not close on the frame boundary"
            );
            ticks.push(next);
            interval_index += 1;
        }
        ticks
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    tick: usize,
    input: f64,
    reference: f64,
}

/// Timestamped normalized input and independent reference records for two
/// complete periodic frames. Build once, then reuse at different cutoffs and
/// scalar precisions.
#[derive(Clone, Debug)]
pub(super) struct TransientRecords {
    schedule: Schedule,
    samples: Vec<Sample>,
}

impl TransientRecords {
    pub(super) fn new(input: &Spectrum, reference: &Spectrum, schedule: Schedule) -> Self {
        let samples = schedule
            .boundaries(TOTAL_TICKS)
            .into_iter()
            .map(|tick| {
                let normalized_time = tick as f64 / TICKS_PER_CUTOFF_CYCLE as f64;
                Sample {
                    tick,
                    input: input.evaluate_real_periodic(normalized_time),
                    reference: reference.evaluate_real_periodic(normalized_time),
                }
            })
            .collect();

        Self { schedule, samples }
    }

    pub(super) fn score<T: Scalar, const N: usize>(
        &self,
        cutoff_hz: f64,
        sigma: f64,
        eta: f64,
    ) -> ShapeMetrics
    where
        IntegrationConfig<T>: Default,
    {
        assert!(cutoff_hz.is_finite() && cutoff_hz > 0.0);
        let first = self
            .samples
            .first()
            .expect("transient records are nonempty");
        let initial_input = from_f64::<T>(first.input);
        let angular_cutoff = T::TAU() * from_f64::<T>(cutoff_hz);
        let mut source = StreamingCore::new(
            Butterworth::<T, N>::new(),
            initial_input,
            angular_cutoff,
            InputModel::Linear,
            IntegrationConfig::default(),
        )
        .unwrap();
        let mut equalized = StreamingCore::<T, _, CoupledState<T, N>>::new(
            EqualizedButterworth::new(from_f64(sigma), from_f64(eta)),
            initial_input,
            angular_cutoff,
            InputModel::Linear,
            IntegrationConfig::default(),
        )
        .unwrap();

        let mut previous = errors_at(
            source.output.to_f64().unwrap(),
            equalized.output.to_f64().unwrap(),
            first.reference,
        );
        let mut errors = WeightedErrors::default();

        for pair in self.samples.windows(2) {
            let [previous_sample, current_sample] = pair else {
                unreachable!("windows of two samples always contain two elements")
            };
            let elapsed_ticks = current_sample.tick - previous_sample.tick;
            let elapsed_cycles = elapsed_ticks as f64 / TICKS_PER_CUTOFF_CYCLE as f64;
            let dt_seconds = elapsed_cycles / cutoff_hz;
            let input = from_f64(current_sample.input);
            let source_output = source
                .update(input, from_f64(dt_seconds))
                .unwrap()
                .to_f64()
                .unwrap();
            let equalized_output = equalized
                .update(input, from_f64(dt_seconds))
                .unwrap()
                .to_f64()
                .unwrap();
            let current = errors_at(source_output, equalized_output, current_sample.reference);

            if current_sample.tick > FRAME_TICKS {
                errors.add_interval(dt_seconds, previous, current);
            }
            previous = current;
        }

        let metrics = errors.metrics();
        assert!(
            errors.denominator.is_finite() && errors.denominator > 0.0,
            "{} reference denominator must be finite and nonzero",
            self.schedule.name()
        );
        assert!(
            metrics.source_error.is_finite() && metrics.equalized_error.is_finite(),
            "{} transient errors must be finite",
            self.schedule.name()
        );
        metrics
    }
}

fn errors_at(source_output: f64, equalized_output: f64, reference: f64) -> SampleErrors {
    SampleErrors {
        source: source_output - reference,
        equalized: equalized_output - reference,
        reference,
    }
}

impl Schedule {
    fn name(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Jitter => "jitter",
            Self::BoundedGaps => "bounded gaps",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_close_both_frames_on_exact_tick_boundaries() {
        let expected_intervals_per_frame = [1_280, 1_600, 1_920];
        for (schedule, expected_count) in
            Schedule::ALL.into_iter().zip(expected_intervals_per_frame)
        {
            let boundaries = schedule.boundaries(TOTAL_TICKS);
            assert_eq!(boundaries.first(), Some(&0));
            assert_eq!(boundaries.last(), Some(&TOTAL_TICKS));
            assert!(boundaries.contains(&FRAME_TICKS));
            assert_eq!(
                boundaries
                    .iter()
                    .filter(|&&tick| tick <= FRAME_TICKS)
                    .count()
                    - 1,
                expected_count
            );
            assert_eq!(boundaries.windows(2).count(), 2 * expected_count);

            let maximum_ticks = boundaries
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .max()
                .unwrap();
            assert_eq!(maximum_ticks, 30);
            let maximum_interval_cycles = maximum_ticks as f64 / TICKS_PER_CUTOFF_CYCLE as f64;
            assert!(maximum_interval_cycles <= 1.0 / (80.0 * 0.7));
        }
    }

    #[test]
    fn warmup_frame_covers_twenty_four_slowest_order_eight_decay_constants() {
        let slowest_decay = (core::f64::consts::PI / 16.0).sin();
        let decay_constants = core::f64::consts::TAU * FRAME_CYCLES as f64 * slowest_decay;
        assert!(decay_constants >= 24.0);
    }

    #[test]
    fn schedule_patterns_are_the_frozen_tick_sequences() {
        assert_eq!(
            Schedule::Uniform
                .boundaries(120)
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .collect::<Vec<_>>(),
            [30, 30, 30, 30]
        );
        assert_eq!(
            Schedule::Jitter
                .boundaries(96)
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .collect::<Vec<_>>(),
            [18, 30, 21, 27]
        );
        assert_eq!(
            Schedule::BoundedGaps
                .boundaries(80)
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .collect::<Vec<_>>(),
            [10, 30, 20, 20]
        );
    }
}
