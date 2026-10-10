//! Gaussian-modulated carrier waveform gate for delayed phase equalization.

#![allow(clippy::cast_precision_loss)]

use super::spectral::Spectrum;
use super::transient::{
    FRAME_CYCLES, SAMPLE_COUNT, SAMPLES_PER_CUTOFF_CYCLE, SCORED_BAND_EDGE, check_order,
};
use super::{
    ORDER_4_ETA, ORDER_4_SIGMA, ORDER_4_TARGET_DELAY, ORDER_8_ETA, ORDER_8_SIGMA,
    ORDER_8_TARGET_DELAY,
};

const SIGMA_CYCLES: f64 = 1.5;
const CARRIER_W: f64 = 0.375;
const CARRIER_PHASE: f64 = 0.37;

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

    const MAX_F64_ERROR: f64 = 0.02;
    const MAX_F32_ERROR: f64 = 0.04;

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
        let (input_4, reference_4, raw_fraction_4) = spectra(4, ORDER_4_TARGET_DELAY);
        let (input_8, reference_8, raw_fraction_8) = spectra(8, ORDER_8_TARGET_DELAY);
        let order_4 = check_order::<4>(
            "Gaussian carrier",
            &input_4,
            &reference_4,
            raw_fraction_4,
            ORDER_4_SIGMA,
            ORDER_4_ETA,
            MAX_F64_ERROR,
            MAX_F32_ERROR,
        );
        let order_8 = check_order::<8>(
            "Gaussian carrier",
            &input_8,
            &reference_8,
            raw_fraction_8,
            ORDER_8_SIGMA,
            ORDER_8_ETA,
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
}
