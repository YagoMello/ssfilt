//! Deterministic streaming waveform gate for the private phase prototype.

use rstest::rstest;

use super::{CoupledState, EqualizedButterworth};
use crate::model::Butterworth;
use crate::scalar::{from_f64, from_usize};
use crate::streaming::StreamingCore;
use crate::{InputModel, IntegrationConfig, Scalar};

const ORDER_4_SIGMA: f64 = 0.774_282_178_267_629_5;
const ORDER_4_ETA: f64 = 0.420_158_711_359_650_1;
const ORDER_4_TARGET_DELAY: f64 = 6.615_894_124_512_561;
const ORDER_8_SIGMA: f64 = 0.631_643_162_371_801_7;
const ORDER_8_ETA: f64 = 0.381_855_079_433_413_2;
const ORDER_8_TARGET_DELAY: f64 = 9.820_666_383_292_307;

const FREQUENCY_RATIOS: [f64; 5] = [
    5.0 / 64.0,
    13.0 / 64.0,
    25.0 / 64.0,
    37.0 / 64.0,
    43.0 / 64.0,
];
const AMPLITUDES: [f64; 5] = [1.0, 0.63, 0.41, 0.29, 0.19];
const PHASES: [f64; 5] = [0.17, -0.73, 1.19, -1.41, 0.48];

#[derive(Clone, Copy, Debug)]
enum Schedule {
    Uniform,
    Jitter,
    BoundedGaps,
}

impl Schedule {
    const ALL: [Self; 3] = [Self::Uniform, Self::Jitter, Self::BoundedGaps];

    fn interval(self, index: usize, shortest_period: f64) -> f64 {
        match self {
            Self::Uniform => shortest_period / 160.0,
            Self::Jitter => {
                const MULTIPLIERS: [f64; 6] = [0.625, 0.875, 1.0, 0.75, 0.9375, 0.6875];
                MULTIPLIERS[index % MULTIPLIERS.len()] * shortest_period / 160.0
            }
            Self::BoundedGaps => {
                const MULTIPLIERS: [f64; 8] = [1.0, 1.0, 4.0, 1.0, 2.0, 1.0, 1.0, 4.0];
                MULTIPLIERS[index % MULTIPLIERS.len()] * shortest_period / 320.0
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ShapeMetrics {
    source_error: f64,
    equalized_error: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct WeightedErrors {
    source_numerator: f64,
    equalized_numerator: f64,
    denominator: f64,
}

impl WeightedErrors {
    fn add_interval(&mut self, dt: f64, previous: SampleErrors, current: SampleErrors) {
        let half_dt = 0.5 * dt;
        self.source_numerator += half_dt
            * previous
                .source
                .mul_add(previous.source, current.source * current.source);
        self.equalized_numerator += half_dt
            * previous
                .equalized
                .mul_add(previous.equalized, current.equalized * current.equalized);
        self.denominator += half_dt
            * previous
                .reference
                .mul_add(previous.reference, current.reference * current.reference);
    }

    fn metrics(self) -> ShapeMetrics {
        ShapeMetrics {
            source_error: (self.source_numerator / self.denominator).sqrt(),
            equalized_error: (self.equalized_numerator / self.denominator).sqrt(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SampleErrors {
    source: f64,
    equalized: f64,
    reference: f64,
}

#[rstest]
#[case::order_4_f64(
    check_multitone::<f64, 4>,
    ORDER_4_SIGMA,
    ORDER_4_ETA,
    ORDER_4_TARGET_DELAY,
    0.02
)]
#[case::order_8_f64(
    check_multitone::<f64, 8>,
    ORDER_8_SIGMA,
    ORDER_8_ETA,
    ORDER_8_TARGET_DELAY,
    0.02
)]
#[case::order_4_f32(
    check_multitone::<f32, 4>,
    ORDER_4_SIGMA,
    ORDER_4_ETA,
    ORDER_4_TARGET_DELAY,
    0.04
)]
#[case::order_8_f32(
    check_multitone::<f32, 8>,
    ORDER_8_SIGMA,
    ORDER_8_ETA,
    ORDER_8_TARGET_DELAY,
    0.04
)]
fn core_band_multitone_meets_the_streaming_shape_gate(
    #[case] check: fn(f64, f64, f64, f64),
    #[case] sigma: f64,
    #[case] eta: f64,
    #[case] target_delay: f64,
    #[case] maximum_error: f64,
) {
    check(sigma, eta, target_delay, maximum_error);
}

fn check_multitone<T: Scalar, const N: usize>(
    sigma: f64,
    eta: f64,
    target_delay: f64,
    maximum_error: f64,
) where
    IntegrationConfig<T>: Default,
{
    for schedule in Schedule::ALL {
        let mut first_cutoff_metrics: Option<f64> = None;
        for cutoff_hz in [1.0, 37.0] {
            let metrics = evaluate::<T, N>(cutoff_hz, schedule, sigma, eta, target_delay);
            assert!(
                metrics.equalized_error <= maximum_error,
                "order {N}, {schedule:?}, cutoff {cutoff_hz}: corrected error {} exceeded {}",
                metrics.equalized_error,
                maximum_error,
            );
            assert!(
                metrics.equalized_error <= 0.25 * metrics.source_error,
                "order {N}, {schedule:?}, cutoff {cutoff_hz}: corrected error {} was not <= 25% of source error {}",
                metrics.equalized_error,
                metrics.source_error,
            );

            if let Some(first) = first_cutoff_metrics {
                assert!(
                    (metrics.equalized_error - first).abs() <= maximum_error * 1.0e-3,
                    "order {N}, {schedule:?}: normalized result changed with physical cutoff"
                );
            } else {
                first_cutoff_metrics = Some(metrics.equalized_error);
            }
        }
    }
}

fn evaluate<T: Scalar, const N: usize>(
    cutoff_hz: f64,
    schedule: Schedule,
    sigma: f64,
    eta: f64,
    target_delay: f64,
) -> ShapeMetrics
where
    IntegrationConfig<T>: Default,
{
    let initial_input = input_at(0.0, cutoff_hz);
    let mut source = StreamingCore::new(
        Butterworth::<T, N>::new(),
        from_f64(initial_input),
        T::TAU() * from_f64(cutoff_hz),
        InputModel::Linear,
        IntegrationConfig::default(),
    )
    .unwrap();
    let mut equalized = StreamingCore::<T, _, CoupledState<T, N>>::new(
        EqualizedButterworth::new(from_f64(sigma), from_f64(eta)),
        from_f64(initial_input),
        T::TAU() * from_f64(cutoff_hz),
        InputModel::Linear,
        IntegrationConfig::default(),
    )
    .unwrap();

    let slowest_decay = (core::f64::consts::PI / (2.0 * from_usize::<f64>(N)))
        .sin()
        .min(sigma);
    let warmup_end = 24.0 / slowest_decay / (core::f64::consts::TAU * cutoff_hz);
    let score_end = warmup_end + 128.0 / cutoff_hz;
    let shortest_period = 1.0 / (FREQUENCY_RATIOS[4] * cutoff_hz);
    let mut time = 0.0;
    let mut schedule_index = 0;

    advance_to(
        warmup_end,
        cutoff_hz,
        schedule,
        shortest_period,
        &mut time,
        &mut schedule_index,
        &mut source,
        &mut equalized,
    );

    let reference = reference_at::<N>(time, cutoff_hz, target_delay);
    let mut previous = SampleErrors {
        source: source.output.to_f64().unwrap() - reference,
        equalized: equalized.output.to_f64().unwrap() - reference,
        reference,
    };
    let mut errors = WeightedErrors::default();

    while time < score_end {
        let nominal_dt = schedule.interval(schedule_index, shortest_period);
        schedule_index += 1;
        let dt = nominal_dt.min(score_end - time);
        time += dt;
        let input = input_at(time, cutoff_hz);
        let source_output = source
            .update(from_f64(input), from_f64(dt))
            .unwrap()
            .to_f64()
            .unwrap();
        let equalized_output = equalized
            .update(from_f64(input), from_f64(dt))
            .unwrap()
            .to_f64()
            .unwrap();
        let reference = reference_at::<N>(time, cutoff_hz, target_delay);
        let current = SampleErrors {
            source: source_output - reference,
            equalized: equalized_output - reference,
            reference,
        };
        errors.add_interval(dt, previous, current);
        previous = current;
    }

    errors.metrics()
}

#[allow(clippy::too_many_arguments)]
fn advance_to<T: Scalar, const N: usize>(
    end: f64,
    cutoff_hz: f64,
    schedule: Schedule,
    shortest_period: f64,
    time: &mut f64,
    schedule_index: &mut usize,
    source: &mut StreamingCore<T, Butterworth<T, N>, [T; N]>,
    equalized: &mut StreamingCore<T, EqualizedButterworth<T, N>, CoupledState<T, N>>,
) {
    while *time < end {
        let nominal_dt = schedule.interval(*schedule_index, shortest_period);
        *schedule_index += 1;
        let dt = nominal_dt.min(end - *time);
        *time += dt;
        let input = from_f64(input_at(*time, cutoff_hz));
        source.update(input, from_f64(dt)).unwrap();
        equalized.update(input, from_f64(dt)).unwrap();
    }
}

fn input_at(time: f64, cutoff_hz: f64) -> f64 {
    FREQUENCY_RATIOS
        .iter()
        .zip(AMPLITUDES)
        .zip(PHASES)
        .map(|((&ratio, amplitude), phase)| {
            amplitude * (core::f64::consts::TAU * cutoff_hz * ratio * time + phase).cos()
        })
        .sum()
}

fn reference_at<const N: usize>(time: f64, cutoff_hz: f64, target_delay: f64) -> f64 {
    FREQUENCY_RATIOS
        .iter()
        .zip(AMPLITUDES)
        .zip(PHASES)
        .map(|((&ratio, amplitude), phase)| {
            let gain = butterworth_gain::<N>(ratio);
            amplitude
                * gain
                * (core::f64::consts::TAU * cutoff_hz * ratio * time + phase - target_delay * ratio)
                    .cos()
        })
        .sum()
}

fn butterworth_gain<const N: usize>(frequency_ratio: f64) -> f64 {
    let mut gain = 1.0;
    for index in 0..N / 2 {
        let angle =
            core::f64::consts::PI * from_usize::<f64>(2 * index + 1) / (2.0 * from_usize::<f64>(N));
        let damping = 2.0 * angle.sin();
        gain /= (1.0 - frequency_ratio * frequency_ratio).hypot(damping * frequency_ratio);
    }
    gain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_respect_the_declared_fine_grid_limit() {
        let shortest_period = 3.25;
        for schedule in Schedule::ALL {
            for index in 0..32 {
                assert!(schedule.interval(index, shortest_period) <= shortest_period / 80.0);
            }
        }
    }

    #[test]
    fn multitone_repeats_after_sixty_four_cutoff_cycles() {
        let cutoff_hz = 3.7;
        for sample in [0.0, 0.13, 1.75, 8.2] {
            let repeated = sample + 64.0 / cutoff_hz;
            assert!((input_at(sample, cutoff_hz) - input_at(repeated, cutoff_hz)).abs() < 2.0e-13);
        }
    }

    #[test]
    fn butterworth_reference_gain_matches_closed_form() {
        for frequency_ratio in FREQUENCY_RATIOS {
            let expected_order_4 = 1.0 / (1.0 + frequency_ratio.powi(8)).sqrt();
            let expected_order_8 = 1.0 / (1.0 + frequency_ratio.powi(16)).sqrt();
            assert!((butterworth_gain::<4>(frequency_ratio) - expected_order_4).abs() < 1.0e-14);
            assert!((butterworth_gain::<8>(frequency_ratio) - expected_order_8).abs() < 1.0e-14);
        }
    }
}
