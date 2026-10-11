//! Deterministic streaming waveform gate for the private phase prototype.

mod bessel_reference;
mod chirp;
mod gaussian;
mod spectral;
mod transient;

use rstest::rstest;

use super::{
    CoupledState, EqualizedButterworth, ORDER_4_ETA, ORDER_4_SIGMA, ORDER_4_TARGET_DELAY,
    ORDER_8_ETA, ORDER_8_SIGMA, ORDER_8_TARGET_DELAY,
};
use crate::model::{Bessel, Butterworth};
use crate::scalar::{from_f64, from_usize};
use crate::streaming::StreamingCore;
use crate::{InputModel, IntegrationConfig, Scalar};

const FREQUENCY_RATIOS: [f64; 5] = [
    5.0 / 64.0,
    13.0 / 64.0,
    25.0 / 64.0,
    37.0 / 64.0,
    43.0 / 64.0,
];
const AMPLITUDES: [f64; 5] = [1.0, 0.63, 0.41, 0.29, 0.19];
const PHASES: [f64; 5] = [0.17, -0.73, 1.19, -1.41, 0.48];
const ORDER_4_SOURCE_TARGET_DELAY: f64 = 3.060_838_533_217_734_4;
const ORDER_8_SOURCE_TARGET_DELAY: f64 = 5.836_514_282_571_954_5;

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
#[allow(clippy::struct_field_names)]
struct ShapeMetrics {
    source_error: f64,
    equalized_error: f64,
    bessel_error: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct WeightedErrors {
    source_numerator: f64,
    equalized_numerator: f64,
    source_denominator: f64,
    equalized_denominator: f64,
    bessel_numerator: f64,
    bessel_denominator: f64,
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
        self.source_denominator += half_dt
            * previous.source_reference.mul_add(
                previous.source_reference,
                current.source_reference * current.source_reference,
            );
        self.equalized_denominator += half_dt
            * previous.equalized_reference.mul_add(
                previous.equalized_reference,
                current.equalized_reference * current.equalized_reference,
            );
        self.bessel_numerator += half_dt
            * previous
                .bessel
                .mul_add(previous.bessel, current.bessel * current.bessel);
        self.bessel_denominator += half_dt
            * previous.bessel_reference.mul_add(
                previous.bessel_reference,
                current.bessel_reference * current.bessel_reference,
            );
    }

    fn metrics(self) -> ShapeMetrics {
        ShapeMetrics {
            source_error: (self.source_numerator / self.source_denominator).sqrt(),
            equalized_error: (self.equalized_numerator / self.equalized_denominator).sqrt(),
            bessel_error: (self.bessel_numerator / self.bessel_denominator).sqrt(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SampleErrors {
    source: f64,
    equalized: f64,
    source_reference: f64,
    equalized_reference: f64,
    bessel: f64,
    bessel_reference: f64,
}

#[rstest]
#[case::order_4_f64(
    check_multitone::<f64, 4>,
    ORDER_4_SIGMA,
    ORDER_4_ETA,
    ORDER_4_SOURCE_TARGET_DELAY,
    ORDER_4_TARGET_DELAY,
    0.02
)]
#[case::order_8_f64(
    check_multitone::<f64, 8>,
    ORDER_8_SIGMA,
    ORDER_8_ETA,
    ORDER_8_SOURCE_TARGET_DELAY,
    ORDER_8_TARGET_DELAY,
    0.02
)]
#[case::order_4_f32(
    check_multitone::<f32, 4>,
    ORDER_4_SIGMA,
    ORDER_4_ETA,
    ORDER_4_SOURCE_TARGET_DELAY,
    ORDER_4_TARGET_DELAY,
    0.04
)]
#[case::order_8_f32(
    check_multitone::<f32, 8>,
    ORDER_8_SIGMA,
    ORDER_8_ETA,
    ORDER_8_SOURCE_TARGET_DELAY,
    ORDER_8_TARGET_DELAY,
    0.04
)]
fn core_band_multitone_meets_the_streaming_shape_gate(
    #[case] check: fn(f64, f64, f64, f64, f64),
    #[case] sigma: f64,
    #[case] eta: f64,
    #[case] source_target_delay: f64,
    #[case] target_delay: f64,
    #[case] maximum_error: f64,
) {
    check(sigma, eta, source_target_delay, target_delay, maximum_error);
}

fn check_multitone<T: Scalar, const N: usize>(
    sigma: f64,
    eta: f64,
    source_target_delay: f64,
    target_delay: f64,
    maximum_error: f64,
) where
    IntegrationConfig<T>: Default,
{
    let mut worst_source_error: f64 = 0.0;
    let mut worst_equalized_error: f64 = 0.0;
    let mut worst_bessel_error: f64 = 0.0;
    let mut worst_ratio: f64 = 0.0;
    for schedule in Schedule::ALL {
        let mut first_cutoff_metrics: Option<f64> = None;
        let mut first_cutoff_bessel_error: Option<f64> = None;
        for cutoff_hz in [1.0, 37.0] {
            let metrics = evaluate::<T, N>(
                cutoff_hz,
                schedule,
                sigma,
                eta,
                source_target_delay,
                target_delay,
            );
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
            if let Some(first) = first_cutoff_bessel_error {
                assert!(
                    (metrics.bessel_error - first).abs() <= maximum_error * 1.0e-3,
                    "order {N}, {schedule:?}: normalized Bessel result changed with physical cutoff"
                );
            } else {
                first_cutoff_bessel_error = Some(metrics.bessel_error);
            }

            worst_source_error = worst_source_error.max(metrics.source_error);
            worst_equalized_error = worst_equalized_error.max(metrics.equalized_error);
            worst_bessel_error = worst_bessel_error.max(metrics.bessel_error);
            worst_ratio = worst_ratio.max(metrics.equalized_error / metrics.source_error);
        }
    }
    println!(
        "Multi-tone gate order {N} {}: worst source/corrected/Bessel errors {:.8}%/{:.8}%/{:.8}%, corrected/source ratio {:.8}%",
        core::any::type_name::<T>(),
        worst_source_error * 100.0,
        worst_equalized_error * 100.0,
        worst_bessel_error * 100.0,
        worst_ratio * 100.0,
    );
}

fn evaluate<T: Scalar, const N: usize>(
    cutoff_hz: f64,
    schedule: Schedule,
    sigma: f64,
    eta: f64,
    source_target_delay: f64,
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
    let mut bessel = StreamingCore::new(
        Bessel::<T, N>::new().expect("matched-order Bessel model is supported"),
        from_f64(initial_input),
        T::TAU() * from_f64(cutoff_hz),
        InputModel::Linear,
        IntegrationConfig::default(),
    )
    .unwrap();

    let slowest_decay = (core::f64::consts::PI / (2.0 * from_usize::<f64>(N)))
        .sin()
        .min(sigma);
    let bessel_target_delay = bessel_reference::minimax_target_delay(N);
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
        &mut bessel,
    );

    let source_reference = reference_at::<N>(time, cutoff_hz, source_target_delay);
    let equalized_reference = reference_at::<N>(time, cutoff_hz, target_delay);
    let bessel_reference = bessel_reference_at::<N>(time, cutoff_hz, bessel_target_delay);
    let mut previous = SampleErrors {
        source: source.output.to_f64().unwrap() - source_reference,
        equalized: equalized.output.to_f64().unwrap() - equalized_reference,
        bessel: bessel.output.to_f64().unwrap() - bessel_reference,
        source_reference,
        equalized_reference,
        bessel_reference,
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
        let bessel_output = bessel
            .update(from_f64(input), from_f64(dt))
            .unwrap()
            .to_f64()
            .unwrap();
        let source_reference = reference_at::<N>(time, cutoff_hz, source_target_delay);
        let equalized_reference = reference_at::<N>(time, cutoff_hz, target_delay);
        let bessel_reference = bessel_reference_at::<N>(time, cutoff_hz, bessel_target_delay);
        let current = SampleErrors {
            source: source_output - source_reference,
            equalized: equalized_output - equalized_reference,
            bessel: bessel_output - bessel_reference,
            source_reference,
            equalized_reference,
            bessel_reference,
        };
        errors.add_interval(dt, previous, current);
        previous = current;
    }

    let metrics = errors.metrics();
    assert!(
        errors.bessel_denominator.is_finite() && errors.bessel_denominator > 0.0,
        "order {N}, {schedule:?}, cutoff {cutoff_hz}: Bessel reference energy must be finite and nontrivial (denominator {})",
        errors.bessel_denominator,
    );
    assert!(metrics.bessel_error.is_finite());
    metrics
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
    bessel: &mut StreamingCore<T, Bessel<T, N>, [T; N]>,
) {
    while *time < end {
        let nominal_dt = schedule.interval(*schedule_index, shortest_period);
        *schedule_index += 1;
        let dt = nominal_dt.min(end - *time);
        *time += dt;
        let input = from_f64(input_at(*time, cutoff_hz));
        source.update(input, from_f64(dt)).unwrap();
        equalized.update(input, from_f64(dt)).unwrap();
        bessel.update(input, from_f64(dt)).unwrap();
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

fn bessel_reference_at<const N: usize>(time: f64, cutoff_hz: f64, target_delay: f64) -> f64 {
    FREQUENCY_RATIOS
        .iter()
        .zip(AMPLITUDES)
        .zip(PHASES)
        .map(|((&ratio, amplitude), phase)| {
            let gain = bessel_reference::magnitude(N, ratio);
            amplitude
                * gain
                * (core::f64::consts::TAU * cutoff_hz * ratio * time + phase - target_delay * ratio)
                    .cos()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_precision_loss)]

    use super::transient::SCORED_BAND_EDGE;
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

    #[test]
    fn frozen_source_targets_match_the_analytic_endpoint_minimax_delay() {
        for (order, frozen_target) in [
            (4, ORDER_4_SOURCE_TARGET_DELAY),
            (8, ORDER_8_SOURCE_TARGET_DELAY),
        ] {
            let at_dc = butterworth_group_delay(order, 0.0);
            let at_band_edge = butterworth_group_delay(order, SCORED_BAND_EDGE);
            assert!(at_band_edge > at_dc);

            let mut previous = at_dc;
            for index in 1..=8_192 {
                let frequency = SCORED_BAND_EDGE * f64::from(index) / 8_192.0;
                let delay = butterworth_group_delay(order, frequency);
                assert!(
                    delay + 1.0e-12 >= previous,
                    "order {order}: group delay was not monotone at W={frequency}"
                );
                previous = delay;
            }

            let endpoint_minimax = f64::midpoint(at_dc, at_band_edge);
            assert!(
                (frozen_target - endpoint_minimax).abs() <= 2.0e-14,
                "order {order}: frozen source target {frozen_target} differs from analytic endpoint midpoint {endpoint_minimax}"
            );
        }
    }

    #[test]
    fn source_and_equalized_errors_use_their_own_reference_energy() {
        let sample = SampleErrors {
            source: 1.0,
            equalized: 1.0,
            bessel: 1.0,
            source_reference: 1.0,
            equalized_reference: 2.0,
            bessel_reference: 4.0,
        };
        let mut errors = WeightedErrors::default();
        errors.add_interval(2.0, sample, sample);

        let metrics = errors.metrics();
        assert!((metrics.source_error - 1.0).abs() < f64::EPSILON);
        assert!((metrics.equalized_error - 0.5).abs() < f64::EPSILON);
        assert!((metrics.bessel_error - 0.25).abs() < f64::EPSILON);
    }

    fn butterworth_group_delay(order: usize, frequency: f64) -> f64 {
        (0..order)
            .map(|index| {
                let angle =
                    core::f64::consts::PI * (2 * index + order + 1) as f64 / (2 * order) as f64;
                let damping = -angle.cos();
                let pole_frequency = angle.sin();
                damping / (damping * damping + (frequency - pole_frequency).powi(2))
            })
            .sum()
    }
}
