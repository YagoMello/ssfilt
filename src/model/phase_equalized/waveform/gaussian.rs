//! Gaussian-modulated carrier waveform gate for delayed phase equalization.

#![allow(clippy::cast_precision_loss)]

use super::spectral::Spectrum;
use super::transient::{FRAME_CYCLES, Schedule, TransientRecords};
use super::{
    ORDER_4_ETA, ORDER_4_SIGMA, ORDER_4_TARGET_DELAY, ORDER_8_ETA, ORDER_8_SIGMA,
    ORDER_8_TARGET_DELAY,
};

const SAMPLES_PER_CUTOFF_CYCLE: usize = 64;
const SAMPLE_COUNT: usize = FRAME_CYCLES * SAMPLES_PER_CUTOFF_CYCLE;
const SIGMA_CYCLES: f64 = 1.5;
const CARRIER_W: f64 = 0.375;
const CARRIER_PHASE: f64 = 0.37;
const SCORED_BAND_EDGE: f64 = 0.7;

/// Build the projected periodic input and independent magnitude-plus-delay
/// reference from the frozen raw Gaussian carrier fixture.
fn spectra(order: usize, target_delay: f64) -> (Spectrum, Spectrum, f64) {
    let raw_samples: Vec<_> = (0..SAMPLE_COUNT)
        .map(|index| raw_periodized_carrier(index as f64 / SAMPLES_PER_CUTOFF_CYCLE as f64))
        .collect();
    let mut input = Spectrum::forward_real(&raw_samples, FRAME_CYCLES as f64);
    let raw_in_band_fraction = input.in_band_energy_fraction(SCORED_BAND_EDGE);
    input.project_in_band(SCORED_BAND_EDGE);
    let reference = input.butterworth_constant_delay(order, target_delay);
    (input, reference, raw_in_band_fraction)
}

/// Periodize the Gaussian-modulated carrier by summing shifted copies.
///
/// The infinite mathematical definition is evaluated with 17 centered copies
/// (`shift = -8..=8`). Over the 20-cycle frame, the omitted copies are more
/// than 100 Gaussian standard deviations away and underflow to zero in f64.
/// Modulating each shifted Gaussian by its local time makes the sum periodic
/// even though the carrier itself is not an integer number of cycles per
/// frame, avoiding a hard window seam.
fn raw_periodized_carrier(normalized_time: f64) -> f64 {
    let frame_cycles = FRAME_CYCLES as f64;
    (-8..=8)
        .map(|shift| {
            let local_time = normalized_time - frame_cycles / 2.0 + f64::from(shift) * frame_cycles;
            let envelope = (-0.5 * (local_time / SIGMA_CYCLES).powi(2)).exp();
            envelope * (core::f64::consts::TAU * CARRIER_W * local_time + CARRIER_PHASE).cos()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntegrationConfig, Scalar};

    const MAX_F64_ERROR: f64 = 0.02;
    const MAX_F32_ERROR: f64 = 0.04;
    const CUTOFFS_HZ: [f64; 2] = [1.0, 37.0];

    #[test]
    fn periodized_gaussian_frame_closes_without_a_window_seam() {
        for time in [0.0, 0.125, 3.5, 9.75, 10.0, 16.25, 19.875] {
            let at_time = raw_periodized_carrier(time);
            let next_frame = raw_periodized_carrier(time + FRAME_CYCLES as f64);
            assert!((at_time - next_frame).abs() < 1.0e-14);
        }

        assert_eq!(SAMPLE_COUNT, 1_280);
        assert_eq!(SAMPLE_COUNT / SAMPLES_PER_CUTOFF_CYCLE, FRAME_CYCLES);
    }

    #[test]
    fn raw_gaussian_spectrum_meets_the_predeclared_band_energy_gate() {
        let (_, _, raw_in_band_fraction) = spectra(4, ORDER_4_TARGET_DELAY);
        assert!(
            raw_in_band_fraction >= 0.99,
            "raw Gaussian carrier has only {:.8}% of two-sided energy inside |W| <= 0.7",
            100.0 * raw_in_band_fraction
        );
    }

    #[test]
    fn gaussian_transient_meets_the_complete_streaming_shape_matrix() {
        let order_4 = check_order::<4>(
            ORDER_4_SIGMA,
            ORDER_4_ETA,
            ORDER_4_TARGET_DELAY,
            MAX_F64_ERROR,
            MAX_F32_ERROR,
        );
        let order_8 = check_order::<8>(
            ORDER_8_SIGMA,
            ORDER_8_ETA,
            ORDER_8_TARGET_DELAY,
            MAX_F64_ERROR,
            MAX_F32_ERROR,
        );

        println!(
            "Gaussian gate: raw in-band energy {:.8}%; order 4 worst f64/f32 errors {:.8}%/{:.8}%, source ratios {:.8}%/{:.8}%; order 8 {:.8}%/{:.8}%, source ratios {:.8}%/{:.8}%",
            order_4.raw_in_band_fraction * 100.0,
            order_4.f64_error * 100.0,
            order_4.f32_error * 100.0,
            order_4.f64_ratio * 100.0,
            order_4.f32_ratio * 100.0,
            order_8.f64_error * 100.0,
            order_8.f32_error * 100.0,
            order_8.f64_ratio * 100.0,
            order_8.f32_ratio * 100.0,
        );
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct WorstMetrics {
        raw_in_band_fraction: f64,
        f64_error: f64,
        f64_ratio: f64,
        f32_error: f64,
        f32_ratio: f64,
    }

    fn check_order<const N: usize>(
        sigma: f64,
        eta: f64,
        target_delay: f64,
        maximum_f64_error: f64,
        maximum_f32_error: f64,
    ) -> WorstMetrics {
        let (input, reference, raw_in_band_fraction) = spectra(N, target_delay);
        assert!(
            raw_in_band_fraction >= 0.99,
            "order {N}: raw two-sided in-band energy fraction was {raw_in_band_fraction}"
        );

        let mut worst = WorstMetrics {
            raw_in_band_fraction,
            ..WorstMetrics::default()
        };
        for schedule in Schedule::ALL {
            let records = TransientRecords::new(&input, &reference, schedule);
            let (f64_error, f64_ratio) =
                check_precision::<f64, N>(&records, sigma, eta, maximum_f64_error, schedule);
            let (f32_error, f32_ratio) =
                check_precision::<f32, N>(&records, sigma, eta, maximum_f32_error, schedule);
            worst.f64_error = worst.f64_error.max(f64_error);
            worst.f64_ratio = worst.f64_ratio.max(f64_ratio);
            worst.f32_error = worst.f32_error.max(f32_error);
            worst.f32_ratio = worst.f32_ratio.max(f32_ratio);
        }
        worst
    }

    fn check_precision<T: Scalar, const N: usize>(
        records: &TransientRecords,
        sigma: f64,
        eta: f64,
        maximum_error: f64,
        schedule: Schedule,
    ) -> (f64, f64)
    where
        IntegrationConfig<T>: Default,
    {
        let metrics = CUTOFFS_HZ.map(|cutoff_hz| {
            let metrics = records.score::<T, N>(cutoff_hz, sigma, eta);
            assert!(
                metrics.source_error > 1.0e-6,
                "order {N}, {schedule:?}, cutoff {cutoff_hz}: source error was trivial ({})",
                metrics.source_error
            );
            assert!(
                metrics.equalized_error <= maximum_error,
                "order {N}, {schedule:?}, cutoff {cutoff_hz}: corrected error {} exceeded {}",
                metrics.equalized_error,
                maximum_error,
            );
            let ratio = metrics.equalized_error / metrics.source_error;
            assert!(
                ratio.is_finite() && ratio <= 0.25,
                "order {N}, {schedule:?}, cutoff {cutoff_hz}: corrected/source ratio {ratio} exceeded 25%"
            );
            metrics
        });

        assert!(
            (metrics[0].equalized_error - metrics[1].equalized_error).abs()
                <= maximum_error * 1.0e-3,
            "order {N}, {schedule:?}: normalized result changed across physical cutoffs"
        );

        let worst_error = metrics
            .iter()
            .map(|metrics| metrics.equalized_error)
            .fold(0.0, f64::max);
        let worst_ratio = metrics
            .iter()
            .map(|metrics| metrics.equalized_error / metrics.source_error)
            .fold(0.0, f64::max);
        (worst_error, worst_ratio)
    }
}
