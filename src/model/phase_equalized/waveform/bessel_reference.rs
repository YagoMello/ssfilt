//! Analytic matched-order Bessel magnitude and constant-delay references.

#![allow(clippy::cast_precision_loss)]

use crate::model::bessel_table;

pub(super) const ORDER_4_TARGET_DELAY: f64 = 2.112_324_335_816_937;
pub(super) const ORDER_8_TARGET_DELAY: f64 = 3.179_617_137_080_76;
pub(super) const CORE_BAND_EDGE: f64 = 0.7;

/// Return the analytic magnitude of the tabulated, magnitude-normalized Bessel
/// cascade at normalized frequency `W = f / cutoff_hz`.
pub(super) fn magnitude(order: usize, frequency: f64) -> f64 {
    assert!(frequency.is_finite() && frequency >= 0.0);
    let prototype = bessel_table::prototype(order).expect("supported Bessel prototype");
    let mut magnitude = if prototype.real_rate > 0.0 {
        prototype.real_rate / prototype.real_rate.hypot(frequency)
    } else {
        1.0
    };

    for (&damping, &frequency_squared) in prototype.damping.iter().zip(prototype.frequency_squared)
    {
        magnitude *= frequency_squared
            / (frequency_squared - frequency * frequency).hypot(damping * frequency);
    }
    magnitude
}

/// Return the analytic group delay with respect to normalized angular
/// frequency `W`, derived from the real and second-order section transfers.
pub(super) fn group_delay(order: usize, frequency: f64) -> f64 {
    assert!(frequency.is_finite() && frequency >= 0.0);
    let prototype = bessel_table::prototype(order).expect("supported Bessel prototype");
    let mut delay = if prototype.real_rate > 0.0 {
        prototype.real_rate
            / prototype
                .real_rate
                .mul_add(prototype.real_rate, frequency * frequency)
    } else {
        0.0
    };

    for (&damping, &frequency_squared) in prototype.damping.iter().zip(prototype.frequency_squared)
    {
        let denominator_real = frequency_squared - frequency * frequency;
        let denominator_imaginary = damping * frequency;
        delay += damping * (frequency_squared + frequency * frequency)
            / denominator_real.mul_add(
                denominator_real,
                denominator_imaginary * denominator_imaginary,
            );
    }
    delay
}

/// Derivative of the analytic group delay, without differentiating wrapped
/// phase. For a second-order section, writing `q = W^2` gives
///
/// `d tau / dW = 2 W b (3 a^2 - a b^2 - 2 a q - q^2) / denom^2`,
///
/// where `denom = (a-q)^2 + b^2 q`.
pub(super) fn group_delay_derivative(order: usize, frequency: f64) -> f64 {
    assert!(frequency.is_finite() && frequency >= 0.0);
    let prototype = bessel_table::prototype(order).expect("supported Bessel prototype");
    let q = frequency * frequency;
    let mut derivative = if prototype.real_rate > 0.0 {
        -2.0 * frequency * prototype.real_rate
            / (prototype.real_rate * prototype.real_rate + q).powi(2)
    } else {
        0.0
    };

    for (&damping, &frequency_squared) in prototype.damping.iter().zip(prototype.frequency_squared)
    {
        let denominator = (frequency_squared - q).powi(2) + damping * damping * q;
        let numerator = 3.0 * frequency_squared * frequency_squared
            - frequency_squared * damping * damping
            - 2.0 * frequency_squared * q
            - q * q;
        derivative += 2.0 * frequency * damping * numerator / denominator.powi(2);
    }
    derivative
}

/// The Bessel minimax constant delay on the validated core band. The dense
/// analytic derivative holdout below establishes that the extrema for orders
/// 4 and 8 are the DC and band-edge values.
pub(super) fn minimax_target_delay(order: usize) -> f64 {
    f64::midpoint(group_delay(order, 0.0), group_delay(order, CORE_BAND_EDGE))
}

#[cfg(test)]
mod tests {
    use super::super::spectral::Spectrum;
    use super::*;

    #[test]
    fn bessel_delay_extrema_are_the_core_band_endpoints() {
        const HOLDOUT_INTERVALS: usize = 16_384;
        const DERIVATIVE_ROUNDOFF: f64 = 2.0e-12;

        for (order, frozen_target) in [(4, ORDER_4_TARGET_DELAY), (8, ORDER_8_TARGET_DELAY)] {
            let at_dc = group_delay(order, 0.0);
            let at_edge = group_delay(order, CORE_BAND_EDGE);
            let mut minimum = f64::INFINITY;
            let mut maximum = f64::NEG_INFINITY;

            // Evaluate the analytic derivative on a 16,385-point holdout,
            // including both endpoints. This checks the endpoint-extrema
            // claim directly instead of estimating delay from wrapped phase.
            for index in 0..=HOLDOUT_INTERVALS {
                let frequency = CORE_BAND_EDGE * index as f64 / HOLDOUT_INTERVALS as f64;
                let delay = group_delay(order, frequency);
                let derivative = group_delay_derivative(order, frequency);
                assert!(delay.is_finite() && derivative.is_finite());
                assert!(
                    derivative <= DERIVATIVE_ROUNDOFF,
                    "order {order}: group-delay derivative {derivative} was positive at W={frequency}"
                );
                assert!(
                    delay <= at_dc + DERIVATIVE_ROUNDOFF && delay >= at_edge - DERIVATIVE_ROUNDOFF,
                    "order {order}: delay {delay} fell outside endpoint extrema at W={frequency}"
                );
                minimum = minimum.min(delay);
                maximum = maximum.max(delay);
            }

            assert!((minimum - at_edge).abs() <= DERIVATIVE_ROUNDOFF);
            assert!((maximum - at_dc).abs() <= DERIVATIVE_ROUNDOFF);
            assert!(at_dc >= at_edge);
            assert!(
                (minimax_target_delay(order) - frozen_target).abs() <= 2.0e-14,
                "order {order}: endpoint minimax target {} differs from frozen target {frozen_target}",
                minimax_target_delay(order)
            );
        }
    }

    #[test]
    fn bessel_magnitude_has_unity_dc_and_the_shared_minus_three_db_cutoff() {
        for order in [4, 8] {
            assert!((magnitude(order, 0.0) - 1.0).abs() <= 2.0e-15);
            assert!((magnitude(order, 1.0) - core::f64::consts::FRAC_1_SQRT_2).abs() <= 2.0e-14);
            assert!(group_delay(order, CORE_BAND_EDGE).is_finite());
        }
    }

    #[test]
    fn report_matched_order_bessel_and_butterworth_magnitude_differences() {
        for order in [4, 8] {
            for frequency in [0.4, 0.7, 1.0] {
                let bessel = magnitude(order, frequency);
                let butterworth = 1.0 / (1.0 + frequency.powf(2.0 * order as f64)).sqrt();
                let difference_db = 20.0 * (bessel / butterworth).log10();
                assert!(difference_db.is_finite());
                println!(
                    "Matched-order magnitude: order {order}, W={frequency:.1}, Bessel={bessel:.12}, Butterworth={butterworth:.12}, Bessel-minus-Butterworth={difference_db:.6} dB"
                );
            }
            let edge_magnitude = magnitude(order, 1.0);
            assert!((edge_magnitude - core::f64::consts::FRAC_1_SQRT_2).abs() <= 2.0e-14);
        }
    }

    #[test]
    fn bessel_constant_delay_oracle_preserves_real_spectrum_symmetry() {
        let samples: Vec<_> = (0..128)
            .map(|index| {
                let phase = core::f64::consts::TAU * f64::from(index) / 128.0;
                phase.cos() + 0.3 * (3.0 * phase + 0.2).sin()
            })
            .collect();
        let input = Spectrum::forward_real(&samples, 8.0);
        let delay = minimax_target_delay(4);
        let reference =
            input.magnitude_plus_constant_delay(delay, |frequency| magnitude(4, frequency));

        for index in 1..input.bins().len() / 2 {
            let mirror = input.bins().len() - index;
            assert!((reference.bins()[index] - reference.bins()[mirror].conj()).norm() <= 1.0e-12);
        }
        assert!(reference.bins()[0].im.abs() <= 1.0e-12);
        let nyquist = input.bins().len() / 2;
        assert!(reference.bins()[nyquist].im.abs() <= 1.0e-15);
        assert!(
            reference
                .bins()
                .iter()
                .all(|bin| bin.re.is_finite() && bin.im.is_finite())
        );
        assert!(reference.bins()[0].im.abs() <= 1.0e-15);
    }
}
