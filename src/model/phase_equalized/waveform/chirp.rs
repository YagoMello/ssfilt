//! Tapered linear-frequency chirp gate for delayed phase equalization.

#![allow(clippy::cast_precision_loss)]

use super::spectral::Spectrum;
use super::transient::{
    FRAME_CYCLES, SAMPLE_COUNT, SAMPLES_PER_CUTOFF_CYCLE, SCORED_BAND_EDGE, WorstMetrics,
    check_order,
};
use super::{
    ORDER_4_ETA, ORDER_4_SIGMA, ORDER_4_TARGET_DELAY, ORDER_8_ETA, ORDER_8_SIGMA,
    ORDER_8_TARGET_DELAY,
};

const W0: f64 = 0.05;
const W1: f64 = 0.65;
const MAX_F64_ERROR: f64 = 0.02;
const MAX_F32_ERROR: f64 = 0.04;

/// Build the projected periodic input and independent magnitude-plus-delay
/// reference from the frozen raw tapered-chirp fixture.
fn spectra(order: usize, target_delay: f64) -> (Spectrum, Spectrum, f64) {
    let raw_samples = raw_samples();
    let mut input = Spectrum::forward_real(&raw_samples, FRAME_CYCLES as f64);
    let raw_in_band_fraction = input.in_band_energy_fraction(SCORED_BAND_EDGE);
    input.project_in_band(SCORED_BAND_EDGE);
    let reference = input.butterworth_constant_delay(order, target_delay);
    (input, reference, raw_in_band_fraction)
}

fn raw_samples() -> Vec<f64> {
    (0..SAMPLE_COUNT)
        .map(|index| raw_tapered_chirp(index as f64 / SAMPLES_PER_CUTOFF_CYCLE as f64))
        .collect()
}

fn raw_tapered_chirp(q: f64) -> f64 {
    let window = (core::f64::consts::PI * q / FRAME_CYCLES as f64)
        .sin()
        .powi(2);
    window * chirp_phase(q).cos()
}

fn chirp_phase(q: f64) -> f64 {
    0.23 + core::f64::consts::TAU * (W0 * q + (W1 - W0) * q.powi(2) / (2.0 * FRAME_CYCLES as f64))
}

/// Instantaneous frequency in cycles per cutoff cycle, obtained by
/// differentiating the frozen phase formula with respect to `q`.
fn instantaneous_frequency(q: f64) -> f64 {
    W0 + (W1 - W0) * q / FRAME_CYCLES as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tapered_chirp_frame_has_a_quiet_periodic_seam() {
        let step = 1.0 / SAMPLES_PER_CUTOFF_CYCLE as f64;
        assert_eq!(SAMPLE_COUNT, 1_280);
        assert_eq!(SAMPLE_COUNT / SAMPLES_PER_CUTOFF_CYCLE, FRAME_CYCLES);
        assert!(raw_tapered_chirp(0.0).abs() < 1.0e-14);
        assert!(raw_tapered_chirp(FRAME_CYCLES as f64).abs() < 1.0e-14);
        assert!(raw_tapered_chirp(step).abs() < 1.0e-4);
        assert!(raw_tapered_chirp(FRAME_CYCLES as f64 - step).abs() < 1.0e-4);

        let samples = raw_samples();
        assert!(samples[0].abs() < 1.0e-14);
        assert!((samples[0] - samples[SAMPLE_COUNT - 1]).abs() < 1.0e-4);
    }

    #[test]
    fn tapered_chirp_instantaneous_frequency_matches_its_linear_sweep() {
        assert!((instantaneous_frequency(0.0) - W0).abs() < 1.0e-15);
        assert!((instantaneous_frequency(FRAME_CYCLES as f64) - W1).abs() < 1.0e-15);

        let mut previous = instantaneous_frequency(0.0);
        for index in 1..=80 {
            let q = FRAME_CYCLES as f64 * f64::from(index) / 80.0;
            let frequency = instantaneous_frequency(q);
            assert!(
                frequency > previous,
                "frequency was not increasing at q={q}"
            );
            previous = frequency;

            let expected = W0 + (W1 - W0) * q / FRAME_CYCLES as f64;
            assert!((frequency - expected).abs() < 1.0e-15);
        }

        let epsilon = 1.0e-5;
        for q in [0.25, 5.0, 10.0, 15.0, 19.75] {
            let measured = (chirp_phase(q + epsilon) - chirp_phase(q - epsilon))
                / (core::f64::consts::TAU * 2.0 * epsilon);
            assert!((measured - instantaneous_frequency(q)).abs() < 1.0e-10);
        }
    }

    #[test]
    fn raw_tapered_chirp_spectrum_meets_the_predeclared_band_energy_gate() {
        let raw_spectrum = Spectrum::forward_real(&raw_samples(), FRAME_CYCLES as f64);
        let raw_in_band_fraction = raw_spectrum.in_band_energy_fraction(SCORED_BAND_EDGE);
        println!(
            "Tapered chirp raw in-band energy: {:.8}%",
            raw_in_band_fraction * 100.0
        );
        assert!(
            raw_in_band_fraction >= 0.99,
            "raw tapered chirp has only {:.8}% of two-sided energy inside |W| <= 0.7",
            100.0 * raw_in_band_fraction
        );
    }

    #[test]
    fn tapered_chirp_transient_meets_the_complete_streaming_shape_matrix() {
        let (input_4, reference_4, raw_fraction_4) = spectra(4, ORDER_4_TARGET_DELAY);
        let (input_8, reference_8, raw_fraction_8) = spectra(8, ORDER_8_TARGET_DELAY);
        let order_4 = check_order::<4>(
            "tapered chirp",
            &input_4,
            &reference_4,
            raw_fraction_4,
            ORDER_4_SIGMA,
            ORDER_4_ETA,
            MAX_F64_ERROR,
            MAX_F32_ERROR,
        );
        let order_8 = check_order::<8>(
            "tapered chirp",
            &input_8,
            &reference_8,
            raw_fraction_8,
            ORDER_8_SIGMA,
            ORDER_8_ETA,
            MAX_F64_ERROR,
            MAX_F32_ERROR,
        );

        print_gate_metrics(order_4, order_8);
    }

    fn print_gate_metrics(order_4: WorstMetrics, order_8: WorstMetrics) {
        println!(
            "Tapered chirp gate: raw in-band energy {:.8}%; order 4 worst f64/f32 errors {:.8}%/{:.8}%, source ratios {:.8}%/{:.8}%; order 8 {:.8}%/{:.8}%, source ratios {:.8}%/{:.8}%",
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
