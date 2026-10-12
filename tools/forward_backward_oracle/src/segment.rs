#![forbid(unsafe_code)]

//! Stable exact interval weights for a complex first-order affine segment.

use rug::{Complex, Float};

/// Exact propagation factors for one interval with a linearly varying input.
///
/// For `z = p * delta`, the endpoint state is
/// `exponential * x + left * u_left + right * u_right`.
#[derive(Clone, Debug)]
pub(crate) struct SegmentWeights {
    pub(crate) exponential: Complex,
    pub(crate) left: Complex,
    pub(crate) right: Complex,
}

/// Computes the exponential and affine-input weights at the precision carried
/// by the least-precise input component.
///
/// For `|z| <= 1/2`, the two entire functions are summed directly, avoiding
/// cancellation in their quotient forms. The first omitted Taylor tail is
/// bounded geometrically: subsequent term ratios are at most `1/4` for
/// `phi1` and `1/6` for `phi2`. The stopping threshold leaves eight guard bits
/// below the requested precision. Outside that disk, complex `expm1` is
/// assembled as
/// `expm1(x) - 2*exp(x)*sin(y/2)^2 + i*exp(x)*sin(y)`.
pub(crate) fn segment_weights(z: &Complex, delta: &Float) -> SegmentWeights {
    let (real_precision, imag_precision) = z.prec();
    let precision = real_precision.min(imag_precision).min(delta.prec());
    let z = Complex::with_val(precision, z);
    let delta = Float::with_val(precision, delta);

    if z.is_zero() {
        let one = Complex::with_val(precision, (1, 0));
        let half_delta = Float::with_val(precision, &delta >> 1);
        let half_weight = Complex::with_val(precision, (half_delta, 0));
        return SegmentWeights {
            exponential: one,
            left: half_weight.clone(),
            right: half_weight,
        };
    }

    let phi1;
    let phi2;
    if is_within_series_disk(&z, precision) {
        (phi1, phi2) = taylor_phi(&z, precision);
    } else {
        let expm1 = complex_expm1(&z, precision);
        let z_squared = complex_mul(&z, &z, precision);
        phi1 = complex_div(&expm1, &z, precision);
        let expm1_minus_z = complex_sub(&expm1, &z, precision);
        phi2 = complex_div(&expm1_minus_z, &z_squared, precision);
    }

    let mut exponential = z.clone();
    exponential.exp_mut();

    let left_factor = complex_sub(&phi1, &phi2, precision);
    SegmentWeights {
        exponential,
        left: complex_scale(&left_factor, &delta, precision),
        right: complex_scale(&phi2, &delta, precision),
    }
}

fn is_within_series_disk(z: &Complex, precision: u32) -> bool {
    let quarter = Float::with_val(precision, 1) >> 2;
    let norm_squared = Float::with_val(precision, z.real() * z.real() + z.imag() * z.imag());
    norm_squared <= quarter
}

fn taylor_phi(z: &Complex, precision: u32) -> (Complex, Complex) {
    let one = Complex::with_val(precision, (1, 0));
    let half = Complex::with_val(precision, (1, 0));
    let half = complex_scale_ratio(&half, 1, 2, precision);

    let mut phi1 = one.clone();
    let mut phi2 = half.clone();
    let mut term1 = one;
    let mut term2 = half;

    let relative_tail = Float::with_val(precision, 1) >> precision.saturating_sub(8).max(1);
    let mut k = 0_u32;
    let max_terms = precision.saturating_mul(2).saturating_add(64);

    loop {
        let denominator1 = Float::with_val(precision, k.saturating_add(2));
        let denominator2 = Float::with_val(precision, k.saturating_add(3));
        term1 = complex_div_real(&complex_mul(&term1, z, precision), &denominator1, precision);
        term2 = complex_div_real(&complex_mul(&term2, z, precision), &denominator2, precision);
        phi1 = complex_add(&phi1, &term1, precision);
        phi2 = complex_add(&phi2, &term2, precision);
        k = k.saturating_add(1);

        let scale = max_float(
            Float::with_val(precision, 1),
            max_float(
                max_component_abs(&phi1, precision),
                max_component_abs(&phi2, precision),
            ),
        );
        let threshold = Float::with_val(precision, &relative_tail * scale);
        if max_component_abs(&term1, precision) <= threshold
            && max_component_abs(&term2, precision) <= threshold
        {
            break;
        }
        assert!(
            k < max_terms,
            "Taylor series failed to reach its precision bound"
        );
    }

    (phi1, phi2)
}

fn complex_expm1(z: &Complex, precision: u32) -> Complex {
    let mut exp_real = z.real().clone();
    exp_real.exp_mut();

    let mut expm1_real = z.real().clone();
    expm1_real.exp_m1_mut();

    let mut sin_imag = z.imag().clone();
    sin_imag.sin_mut();

    let mut half_imag = Float::with_val(precision, z.imag() >> 1);
    half_imag.sin_mut();
    let two_sin_half_squared = Float::with_val(precision, &half_imag * &half_imag) << 1;

    let real = Float::with_val(precision, expm1_real - &exp_real * &two_sin_half_squared);
    let imag = Float::with_val(precision, exp_real * sin_imag);
    Complex::with_val(precision, (real, imag))
}

fn complex_add(a: &Complex, b: &Complex, precision: u32) -> Complex {
    let real = Float::with_val(precision, a.real() + b.real());
    let imag = Float::with_val(precision, a.imag() + b.imag());
    Complex::with_val(precision, (real, imag))
}

fn complex_sub(a: &Complex, b: &Complex, precision: u32) -> Complex {
    let real = Float::with_val(precision, a.real() - b.real());
    let imag = Float::with_val(precision, a.imag() - b.imag());
    Complex::with_val(precision, (real, imag))
}

fn complex_mul(a: &Complex, b: &Complex, precision: u32) -> Complex {
    let real = Float::with_val(precision, a.real() * b.real() - a.imag() * b.imag());
    let imag = Float::with_val(precision, a.real() * b.imag() + a.imag() * b.real());
    Complex::with_val(precision, (real, imag))
}

fn complex_div(a: &Complex, b: &Complex, precision: u32) -> Complex {
    Complex::with_val(precision, a / b)
}

fn complex_div_real(a: &Complex, b: &Float, precision: u32) -> Complex {
    let real = Float::with_val(precision, a.real() / b);
    let imag = Float::with_val(precision, a.imag() / b);
    Complex::with_val(precision, (real, imag))
}

fn complex_scale(a: &Complex, scale: &Float, precision: u32) -> Complex {
    let real = Float::with_val(precision, a.real() * scale);
    let imag = Float::with_val(precision, a.imag() * scale);
    Complex::with_val(precision, (real, imag))
}

fn complex_scale_ratio(a: &Complex, numerator: u32, denominator: u32, precision: u32) -> Complex {
    let numerator = Float::with_val(precision, numerator);
    let denominator = Float::with_val(precision, denominator);
    let scale = Float::with_val(precision, numerator / denominator);
    complex_scale(a, &scale, precision)
}

fn max_component_abs(value: &Complex, precision: u32) -> Float {
    max_float(
        Float::with_val(precision, value.real().clone().abs()),
        Float::with_val(precision, value.imag().clone().abs()),
    )
}

fn max_float(a: Float, b: Float) -> Float {
    if a >= b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::{SegmentWeights, segment_weights};
    use rug::{Complex, Float};

    fn ratio(precision: u32, numerator: u32, denominator: u32) -> Float {
        Float::with_val(
            precision,
            Float::with_val(precision, numerator) / Float::with_val(precision, denominator),
        )
    }

    fn power_of_two(precision: u32, negative_exponent: u32) -> Float {
        Float::with_val(precision, 1) >> negative_exponent
    }

    fn complex_max_abs(value: &Complex, precision: u32) -> Float {
        let real = Float::with_val(precision, value.real().clone().abs());
        let imag = Float::with_val(precision, value.imag().clone().abs());
        if real >= imag { real } else { imag }
    }

    fn assert_close(a: &Complex, b: &Complex, precision: u32, tolerance_bits: u32) {
        let difference = Complex::with_val(precision, a - b);
        let error = complex_max_abs(&difference, precision);
        let tolerance = power_of_two(precision, tolerance_bits);
        assert!(
            error <= tolerance,
            "complex values differ by {error}; expected at most {tolerance}"
        );
    }

    fn add(a: &Complex, b: &Complex, precision: u32) -> Complex {
        Complex::with_val(precision, a + b)
    }

    fn sub(a: &Complex, b: &Complex, precision: u32) -> Complex {
        Complex::with_val(precision, a - b)
    }

    fn div(a: &Complex, b: &Complex, precision: u32) -> Complex {
        Complex::with_val(precision, a / b)
    }

    fn direct_closed_forms(z: &Complex, delta: &Float, precision: u32) -> SegmentWeights {
        let z = Complex::with_val(precision, z);
        let delta = Float::with_val(precision, delta);
        let mut exponential = z.clone();
        exponential.exp_mut();
        let one = Complex::with_val(precision, (1, 0));
        let expm1 = sub(&exponential, &one, precision);
        let phi1 = div(&expm1, &z, precision);
        let z_squared = Complex::with_val(precision, &z * &z);
        let expm1_minus_z = sub(&expm1, &z, precision);
        let phi2 = div(&expm1_minus_z, &z_squared, precision);
        let left_factor = sub(&phi1, &phi2, precision);
        SegmentWeights {
            exponential,
            left: Complex::with_val(precision, &left_factor * &delta),
            right: Complex::with_val(precision, &phi2 * &delta),
        }
    }

    fn assert_weights_close(
        actual: &SegmentWeights,
        expected: &SegmentWeights,
        precision: u32,
        tolerance_bits: u32,
    ) {
        assert_close(
            &actual.exponential,
            &expected.exponential,
            precision,
            tolerance_bits,
        );
        assert_close(&actual.left, &expected.left, precision, tolerance_bits);
        assert_close(&actual.right, &expected.right, precision, tolerance_bits);
    }

    #[test]
    fn zero_has_analytic_limits() {
        for precision in [192, 256] {
            let delta = ratio(precision, 13, 8);
            let z = Complex::with_val(precision, (0, 0));
            let weights = segment_weights(&z, &delta);
            let half_delta = Float::with_val(precision, &delta >> 1);

            assert_eq!(weights.exponential, (1, 0));
            assert_eq!(weights.left, (half_delta.clone(), 0));
            assert_eq!(weights.right, (half_delta, 0));
            assert_eq!(weights.exponential.prec(), (precision, precision));
        }
    }

    #[test]
    fn agrees_with_closed_forms_on_both_sides_of_series_threshold() {
        for precision in [192, 256] {
            let reference_precision = precision + 128;
            let delta = ratio(precision, 7, 4);
            for (real, real_denominator, imag, imag_denominator) in
                [(1, 4, 1, 8), (1, 2, 0, 1), (9, 16, 0, 1), (3, 4, 1, 2)]
            {
                let z = Complex::with_val(
                    precision,
                    (
                        ratio(precision, real, real_denominator),
                        ratio(precision, imag, imag_denominator),
                    ),
                );
                let weights = segment_weights(&z, &delta);
                let reference = direct_closed_forms(&z, &delta, reference_precision);
                assert_weights_close(&weights, &reference, reference_precision, precision - 24);
            }
        }
    }

    #[test]
    fn conjugating_the_pole_conjugates_every_weight() {
        for precision in [192, 256] {
            let delta = ratio(precision, 5, 3);
            for z in [
                Complex::with_val(precision, (ratio(precision, 1, 4), ratio(precision, 1, 8))),
                Complex::with_val(precision, (ratio(precision, 3, 4), ratio(precision, 1, 2))),
            ] {
                let positive = segment_weights(&z, &delta);
                let negative = segment_weights(&z.clone().conj(), &delta);
                let input_precision = precision;
                let check_precision = input_precision + 64;
                assert_close(
                    &negative.exponential,
                    &positive.exponential.clone().conj(),
                    check_precision,
                    input_precision - 24,
                );
                assert_close(
                    &negative.left,
                    &positive.left.clone().conj(),
                    check_precision,
                    input_precision - 24,
                );
                assert_close(
                    &negative.right,
                    &positive.right.clone().conj(),
                    check_precision,
                    input_precision - 24,
                );
            }
        }
    }

    #[test]
    fn affine_weights_sum_to_delta_times_phi1() {
        for precision in [192, 256] {
            let delta = ratio(precision, 11, 8);
            let one = Complex::with_val(precision + 64, (1, 0));
            for z in [
                Complex::with_val(precision, (0, 0)),
                Complex::with_val(precision, (ratio(precision, 1, 4), ratio(precision, 1, 8))),
                Complex::with_val(precision, (ratio(precision, 3, 4), ratio(precision, 1, 2))),
            ] {
                let weights = segment_weights(&z, &delta);
                let sum = add(&weights.left, &weights.right, precision + 64);
                let expected = if z.is_zero() {
                    Complex::with_val(precision + 64, &delta)
                } else {
                    let mut exponential = Complex::with_val(precision + 64, &z);
                    exponential.exp_mut();
                    let expm1 = sub(&exponential, &one, precision + 64);
                    let phi1 = div(
                        &expm1,
                        &Complex::with_val(precision + 64, &z),
                        precision + 64,
                    );
                    Complex::with_val(precision + 64, &phi1 * &delta)
                };
                assert_close(&sum, &expected, precision + 64, precision - 24);
            }
        }
    }

    #[test]
    fn independent_calls_converge_when_requested_precision_increases() {
        let z_256 = Complex::with_val(256, (ratio(256, 3, 4), ratio(256, 1, 2)));
        let delta_256 = ratio(256, 13, 8);
        let z_192 = Complex::with_val(192, &z_256);
        let delta_192 = Float::with_val(192, &delta_256);
        let weights_192 = segment_weights(&z_192, &delta_192);
        let weights_256 = segment_weights(&z_256, &delta_256);

        assert_weights_close(&weights_192, &weights_256, 256, 160);
        assert_eq!(weights_192.exponential.prec(), (192, 192));
        assert_eq!(weights_256.exponential.prec(), (256, 256));
    }
}
