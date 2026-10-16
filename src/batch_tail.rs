//! Analytic constant-input continuation for the private batch trajectory pass.

use crate::Scalar;
use crate::model::{Butterworth, ContinuousModel};
use crate::scalar::from_f64;

/// Returns the exact reverse-pass state induced by the continuing forward tail.
///
/// The supported causal Butterworth orders are one, two, four, and eight.
/// Unsupported orders and any unrepresentable intermediate return `None`.
pub(crate) fn analytic_tail_state<const N: usize, T: Scalar>(
    model: &Butterworth<T, N>,
    final_state: &[T; N],
    final_input: T,
) -> Option<[T; N]> {
    if N != 1 && N != 2 && N != 4 && N != 8 {
        return None;
    }
    if !final_input.is_finite() || final_state.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let equilibrium = model.equilibrium(final_input);
    let mut deviation = [T::zero(); N];
    let mut is_equilibrium = true;
    for index in 0..N {
        if !equilibrium[index].is_finite() {
            return None;
        }
        deviation[index] = checked_sub(final_state[index], equilibrium[index])?;
        is_equilibrium &= deviation[index] == T::zero();
    }

    // Preserve the topology's exact constant-input equilibrium without
    // introducing roundoff from a matrix-vector product.
    if is_equilibrium {
        return Some(equilibrium);
    }

    if N == 1 {
        let half_deviation = checked_mul(from_f64::<T>(0.5), deviation[0])?;
        let mut state = equilibrium;
        state[0] = checked_add(equilibrium[0], half_deviation)?;
        return Some(state);
    }

    let map = tail_map(model)?;
    let mut state = equilibrium;
    for row in 0..N {
        let mut correction = T::zero();
        for column in 0..N {
            let product = checked_mul(map[row][column], deviation[column])?;
            correction = checked_add(correction, product)?;
        }
        state[row] = checked_add(equilibrium[row], correction)?;
    }
    Some(state)
}

/// Builds the full fixed-size Sylvester map for an even Butterworth cascade.
///
/// Each block is solved in ascending row-section order and descending
/// column-section order. This follows the lower-bidiagonal coupling in the
/// realized cascade and needs at most the supported order-eight matrix.
fn tail_map<const N: usize, T: Scalar>(model: &Butterworth<T, N>) -> Option<[[T; N]; N]> {
    if N != 2 && N != 4 && N != 8 {
        return None;
    }

    let section_count = N / 2;
    let damping = model.damping();
    if damping.len() != section_count || damping.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mut map = [[T::zero(); N]; N];
    for row_section in 0..section_count {
        for column_section in (0..section_count).rev() {
            let mut d00 = T::zero();
            let d01 = T::zero();
            let mut d10 = T::zero();
            let mut d11 = T::zero();

            if row_section == 0 && column_section + 1 == section_count {
                // -B*C, where the input enters the first section's derivative
                // and the output is the last section's first state.
                d10 = -T::one();
            }

            if row_section > 0 {
                let previous_row = 2 * (row_section - 1);
                let previous_column = 2 * column_section;
                d10 = checked_sub(d10, map[previous_row][previous_column])?;
                d11 = checked_sub(d11, map[previous_row][previous_column + 1])?;
            }

            if column_section + 1 < section_count {
                let next_row = 2 * row_section;
                let next_column = 2 * (column_section + 1);
                d00 = checked_sub(d00, map[next_row][next_column + 1])?;
                d10 = checked_sub(d10, map[next_row + 1][next_column + 1])?;
            }

            let a = damping[row_section];
            let b = damping[column_section];
            let [x00, x01, x10, x11] = solve_sylvester_block(a, b, d00, d01, d10, d11)?;
            let row = 2 * row_section;
            let column = 2 * column_section;
            map[row][column] = x00;
            map[row][column + 1] = x01;
            map[row + 1][column] = x10;
            map[row + 1][column + 1] = x11;
        }
    }

    Some(map)
}

/// Solves `A(a) * X + X * A(b) = D` for the realized real biquad blocks.
fn solve_sylvester_block<T: Scalar>(a: T, b: T, d00: T, d01: T, d10: T, d11: T) -> Option<[T; 4]> {
    let sum = checked_add(a, b)?;
    if sum <= T::zero() {
        return None;
    }

    let x11 = checked_div(checked_sub(d00, d11)?, sum)?;
    let a_d00 = checked_mul(a, d00)?;
    let x01_numerator = checked_add(checked_add(d10, d01)?, a_d00)?;
    let x01 = checked_div(-x01_numerator, sum)?;
    let x10 = checked_add(d00, x01)?;
    let b_x01 = checked_mul(b, x01)?;
    let x00 = checked_add(checked_sub(d01, x11)?, b_x01)?;

    Some([x00, x01, x10, x11])
}

fn checked_add<T: Scalar>(left: T, right: T) -> Option<T> {
    finite(left + right)
}

fn checked_sub<T: Scalar>(left: T, right: T) -> Option<T> {
    finite(left - right)
}

fn checked_mul<T: Scalar>(left: T, right: T) -> Option<T> {
    finite(left * right)
}

fn checked_div<T: Scalar>(numerator: T, denominator: T) -> Option<T> {
    finite(numerator / denominator)
}

fn finite<T: Scalar>(value: T) -> Option<T> {
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use crate::Scalar;
    use crate::model::{Butterworth, ContinuousModel};
    use crate::scalar::from_f64;

    use super::{analytic_tail_state, tail_map};

    #[test]
    fn order_one_tail_matches_closed_form() {
        let model = Butterworth::<f64, 1>::new();
        let result = analytic_tail_state(&model, &[2.0], 0.5).unwrap();
        assert_eq!(result, [1.25]);
    }

    #[test]
    fn order_two_map_matches_closed_form_anchor() {
        let model = Butterworth::<f64, 2>::new();
        let map = tail_map(&model).unwrap();
        let damping = model.damping()[0];
        let off_diagonal = 1.0 / (2.0 * damping);

        assert!((map[0][0] - 0.5).abs() < 1.0e-15);
        assert!((map[0][1] - off_diagonal).abs() < 1.0e-15);
        assert!((map[1][0] - off_diagonal).abs() < 1.0e-15);
        assert_eq!(map[1][1], 0.0);
    }

    #[test]
    fn order_two_tail_matches_closed_form_anchor() {
        let model = Butterworth::<f64, 2>::new();
        let result = analytic_tail_state(&model, &[1.0, 2.0], 0.0).unwrap();
        let off_diagonal = 1.0 / (2.0 * model.damping()[0]);
        assert!((result[0] - (0.5 + 2.0 * off_diagonal)).abs() < 1.0e-15);
        assert!((result[1] - off_diagonal).abs() < 1.0e-15);
    }

    #[test]
    fn order_four_final_output_row_matches_independent_dense_reference() {
        let model = Butterworth::<f64, 4>::new();
        let map = tail_map(&model).unwrap();
        // Frozen from an independent dense 16-by-16 solve of A*M + M*A = -B*C.
        let expected = [
            0.603_553_390_593_273_7,
            0.326_640_741_219_094_14,
            -0.103_553_390_593_273_76,
            0.191_341_716_182_544_9,
        ];
        for (actual, expected) in map[2].iter().zip(expected) {
            assert!((*actual - expected).abs() < 2.0e-14);
        }
        assert!((map[2][0] - map[0][2]).abs() > 0.2);
    }

    #[test]
    fn sylvester_residual_is_small_for_f64_orders_two_four_and_eight() {
        check_sylvester_residual::<2, f64>(1.0e-12);
        check_sylvester_residual::<4, f64>(1.0e-12);
        check_sylvester_residual::<8, f64>(1.0e-12);
    }

    #[test]
    fn sylvester_residual_is_small_for_f32_orders_two_four_and_eight() {
        check_sylvester_residual::<2, f32>(2.0e-5);
        check_sylvester_residual::<4, f32>(2.0e-5);
        check_sylvester_residual::<8, f32>(2.0e-5);
    }

    fn check_sylvester_residual<const N: usize, T: Scalar>(tolerance: f64) {
        let model = Butterworth::<T, N>::new();
        let map = tail_map(&model).unwrap();
        let tolerance = from_f64::<T>(tolerance);
        let mut largest_residual = T::zero();

        for row in 0..N {
            for column in 0..N {
                let mut residual = T::zero();
                for (inner, map_row) in map.iter().enumerate() {
                    residual = residual + matrix_element(&model, row, inner) * map_row[column];
                    residual = residual + map[row][inner] * matrix_element(&model, inner, column);
                }
                if row == 1 && column == N - 2 {
                    residual = residual + T::one();
                }
                largest_residual = largest_residual.max(residual.abs());
            }
        }

        assert!(
            largest_residual <= tolerance,
            "order {N} Sylvester residual {largest_residual:?} exceeds {tolerance:?}"
        );
    }

    fn matrix_element<const N: usize, T: Scalar>(
        model: &Butterworth<T, N>,
        row: usize,
        column: usize,
    ) -> T {
        let row_section = row / 2;
        let column_section = column / 2;
        let row_component = row % 2;
        let column_component = column % 2;
        if row_section == column_section {
            match (row_component, column_component) {
                (0, 1) => T::one(),
                (1, 0) => -T::one(),
                (1, 1) => -model.damping()[row_section],
                _ => T::zero(),
            }
        } else if row_component == 1 && column_component == 0 && row_section == column_section + 1 {
            T::one()
        } else {
            T::zero()
        }
    }

    #[test]
    fn constant_equilibrium_is_returned_exactly() {
        check_exact_equilibrium::<1, f64>(3.25);
        check_exact_equilibrium::<2, f64>(-1.5);
        check_exact_equilibrium::<4, f64>(0.75);
        check_exact_equilibrium::<8, f64>(-0.125);
        check_exact_equilibrium::<4, f32>(12.5);
    }

    fn check_exact_equilibrium<const N: usize, T: Scalar>(input: T) {
        let model = Butterworth::<T, N>::new();
        let equilibrium = model.equilibrium(input);
        assert_eq!(
            analytic_tail_state(&model, &equilibrium, input).unwrap(),
            equilibrium
        );
    }

    #[test]
    fn nonfinite_values_and_overflow_return_none() {
        let order_one = Butterworth::<f64, 1>::new();
        assert!(analytic_tail_state(&order_one, &[0.0], f64::NAN).is_none());
        assert!(analytic_tail_state(&order_one, &[f64::INFINITY], 0.0).is_none());
        assert!(analytic_tail_state(&order_one, &[f64::MAX], -f64::MAX).is_none());

        let order_four = Butterworth::<f64, 4>::new();
        assert!(analytic_tail_state(&order_four, &[f64::MAX; 4], 0.0).is_none());
    }
}
