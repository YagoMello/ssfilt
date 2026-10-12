use rug::{Complex, Float};

use crate::model::ButterworthModes;
use crate::record::{NormalizedRecord, RepresentedRecord};
use crate::segment::segment_weights;

#[derive(Clone, Debug)]
pub struct OracleOutput {
    precision: u32,
    values: Vec<Float>,
    maximum_imaginary_residual: Float,
}

impl OracleOutput {
    #[must_use]
    pub const fn precision(&self) -> u32 {
        self.precision
    }

    #[must_use]
    pub fn values(&self) -> &[Float] {
        &self.values
    }

    #[must_use]
    pub const fn maximum_imaginary_residual(&self) -> &Float {
        &self.maximum_imaginary_residual
    }
}

/// Evaluates the independent finite-record forward-backward convolution at
/// every supplied knot.
///
/// The represented record is promoted afresh at `precision`; its cutoff,
/// intervals, and samples are treated as exact binary values. No candidate
/// model, integrator, or coefficient generator participates.
#[must_use]
pub fn evaluate(order: usize, record: &RepresentedRecord, precision: u32) -> OracleOutput {
    let modes = ButterworthModes::new(order, precision);
    let normalized = record.normalize(precision);
    evaluate_normalized(&modes, &normalized)
}

fn evaluate_normalized(modes: &ButterworthModes, record: &NormalizedRecord) -> OracleOutput {
    assert_eq!(
        record.intervals.len() + 1,
        record.samples.len(),
        "normalized record shape must already be valid"
    );
    let precision = modes.precision();
    let mut output = (0..record.samples.len())
        .map(|_| Complex::new(precision))
        .collect::<Vec<_>>();

    for (pole, coefficient) in modes.poles().iter().zip(modes.kernel_coefficients()) {
        let mut left_state = Complex::with_val(precision, &record.samples[0]);
        left_state = -left_state;
        left_state /= pole;
        add_modal_contribution(&mut output[0], coefficient, &left_state);

        for index in 0..record.intervals.len() {
            let weights = weights_for_interval(pole, &record.intervals[index]);
            let mut next = left_state;
            next *= &weights.exponential;
            add_scaled(&mut next, &weights.left, &record.samples[index]);
            add_scaled(&mut next, &weights.right, &record.samples[index + 1]);
            left_state = next;
            add_modal_contribution(&mut output[index + 1], coefficient, &left_state);
        }

        let last = record.samples.len() - 1;
        let mut right_state = Complex::with_val(precision, &record.samples[last]);
        right_state = -right_state;
        right_state /= pole;
        add_modal_contribution(&mut output[last], coefficient, &right_state);

        for index in (0..record.intervals.len()).rev() {
            let weights = weights_for_interval(pole, &record.intervals[index]);
            let mut previous = right_state;
            previous *= &weights.exponential;
            add_scaled(&mut previous, &weights.right, &record.samples[index]);
            add_scaled(&mut previous, &weights.left, &record.samples[index + 1]);
            right_state = previous;
            add_modal_contribution(&mut output[index], coefficient, &right_state);
        }
    }

    let mut maximum_imaginary_residual = Float::new(precision);
    let values = output
        .into_iter()
        .map(|value| {
            let imaginary = Float::with_val(precision, value.imag().clone().abs());
            update_maximum_imaginary_residual(&mut maximum_imaginary_residual, imaginary);
            value.real().clone()
        })
        .collect();

    OracleOutput {
        precision,
        values,
        maximum_imaginary_residual,
    }
}

fn update_maximum_imaginary_residual(maximum: &mut Float, candidate: Float) {
    if maximum.is_finite() && (!candidate.is_finite() || candidate > *maximum) {
        *maximum = candidate;
    }
}

fn weights_for_interval(
    pole: &Complex,
    normalized_interval: &Float,
) -> crate::segment::SegmentWeights {
    let precision = pole.prec().0.min(normalized_interval.prec());
    let mut argument = Complex::with_val(precision, pole);
    argument *= normalized_interval;
    segment_weights(&argument, normalized_interval)
}

fn add_scaled(accumulator: &mut Complex, weight: &Complex, sample: &Float) {
    let mut term = weight.clone();
    term *= sample;
    *accumulator += term;
}

fn add_modal_contribution(accumulator: &mut Complex, coefficient: &Complex, state: &Complex) {
    let mut term = coefficient.clone();
    term *= state;
    *accumulator += term;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RepresentedRecord;

    fn maximum_difference(left: &[Float], right: &[Float], precision: u32) -> Float {
        left.iter()
            .zip(right)
            .map(|(left, right)| Float::with_val(precision, left - right).abs())
            .fold(Float::new(precision), |maximum, difference| {
                if difference > maximum {
                    difference
                } else {
                    maximum
                }
            })
    }

    #[test]
    fn non_finite_imaginary_residuals_remain_observable() {
        let precision = 192;
        let mut maximum = Float::new(precision);

        update_maximum_imaginary_residual(&mut maximum, Float::with_val(precision, 2));
        update_maximum_imaginary_residual(&mut maximum, Float::with_val(precision, 1));
        assert_eq!(maximum, Float::with_val(precision, 2));

        update_maximum_imaginary_residual(&mut maximum, Float::with_val(precision, f64::NAN));
        assert!(maximum.is_nan());

        update_maximum_imaginary_residual(&mut maximum, Float::with_val(precision, 3));
        assert!(maximum.is_nan());

        let mut infinite = Float::new(precision);
        update_maximum_imaginary_residual(&mut infinite, Float::with_val(precision, f64::INFINITY));
        assert!(infinite.is_infinite());
    }

    #[test]
    fn constant_half_lines_and_segments_have_unit_dc_gain() {
        for precision in [192, 256] {
            for order in [1, 2, 4, 8] {
                let record = RepresentedRecord::from_f64(
                    37.0,
                    vec![0.001, 0.003, 0.000_125, 0.002],
                    vec![-1.25; 5],
                )
                .unwrap();
                let output = evaluate(order, &record, precision);
                let expected = vec![Float::with_val(precision, -1.25); 5];
                let error = maximum_difference(output.values(), &expected, precision);
                assert!(error < Float::with_val(precision, 1.0e-50));
                assert!(output.maximum_imaginary_residual() < &Float::with_val(precision, 1.0e-50));
            }
        }
    }

    #[test]
    fn reversing_the_record_reverses_the_oracle_output() {
        let intervals = vec![0.03, 0.002, 0.017, 0.011];
        let samples = vec![-0.4, 0.7, 1.2, -0.3, 0.1];
        let forward = RepresentedRecord::from_f64(1.0, intervals.clone(), samples.clone()).unwrap();

        let mut reverse_intervals = intervals;
        reverse_intervals.reverse();
        let mut reverse_samples = samples;
        reverse_samples.reverse();
        let reverse = RepresentedRecord::from_f64(1.0, reverse_intervals, reverse_samples).unwrap();

        for order in [1, 2, 4, 8] {
            let forward_output = evaluate(order, &forward, 192);
            let reverse_output = evaluate(order, &reverse, 192);
            let reversed = reverse_output
                .values()
                .iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>();
            let error = maximum_difference(forward_output.values(), &reversed, 192);
            assert!(error < Float::with_val(192, 1.0e-48));
        }
    }

    #[test]
    fn fresh_192_and_256_bit_runs_converge() {
        let record = RepresentedRecord::from_f32(
            37.0,
            vec![0.001, 0.000_25, 0.002, 0.000_75],
            vec![-0.4, 0.7, 1.2, -0.3, 0.1],
        )
        .unwrap();

        for order in [1, 2, 4, 8] {
            let low = evaluate(order, &record, 192);
            let high = evaluate(order, &record, 256);
            let error = maximum_difference(low.values(), high.values(), 256);
            assert!(error < Float::with_val(256, 1.0e-50));
        }
    }

    #[test]
    fn singleton_record_uses_both_constant_half_lines() {
        let record = RepresentedRecord::from_f64(1.0, vec![], vec![0.375]).unwrap();
        for order in [1, 2, 4, 8] {
            let output = evaluate(order, &record, 192);
            assert_eq!(output.values().len(), 1);
            let error = Float::with_val(192, &output.values()[0] - 0.375).abs();
            assert!(error < Float::with_val(192, 1.0e-50));
        }
    }

    #[test]
    fn exact_collinear_refinement_preserves_the_oracle_at_original_knots() {
        for precision in [192, 256] {
            let base = NormalizedRecord {
                intervals: vec![
                    Float::with_val(precision, 1) / 5,
                    Float::with_val(precision, 7) / 13,
                    Float::with_val(precision, 3) / 8,
                ],
                samples: vec![
                    Float::with_val(precision, -2) / 5,
                    Float::with_val(precision, 7) / 9,
                    Float::with_val(precision, 6) / 5,
                    Float::with_val(precision, -3) / 10,
                ],
            };

            let mut refined_intervals = Vec::new();
            let mut refined_samples = vec![base.samples[0].clone()];
            for index in 0..base.intervals.len() {
                let third = Float::with_val(precision, &base.intervals[index] / 3);
                let mut final_third = base.intervals[index].clone();
                let mut first_two = third.clone();
                first_two *= 2;
                final_third -= first_two;
                refined_intervals.extend([third.clone(), third, final_third]);

                let difference =
                    Float::with_val(precision, &base.samples[index + 1] - &base.samples[index]);
                let mut first_offset = difference.clone();
                first_offset /= 3;
                let mut first = base.samples[index].clone();
                first += first_offset;

                let mut second_offset = difference;
                second_offset *= 2;
                second_offset /= 3;
                let mut second = base.samples[index].clone();
                second += second_offset;
                refined_samples.extend([first, second, base.samples[index + 1].clone()]);
            }
            let refined = NormalizedRecord {
                intervals: refined_intervals,
                samples: refined_samples,
            };

            for order in [1, 2, 4, 8] {
                let modes = ButterworthModes::new(order, precision);
                let base_output = evaluate_normalized(&modes, &base);
                let refined_output = evaluate_normalized(&modes, &refined);
                let original_knots = refined_output
                    .values()
                    .iter()
                    .step_by(3)
                    .cloned()
                    .collect::<Vec<_>>();
                let error = maximum_difference(base_output.values(), &original_knots, precision);
                assert!(error < Float::with_val(precision, 1.0e-50));
            }
        }
    }
}
