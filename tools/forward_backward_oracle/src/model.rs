use rug::{Complex, Float, float::Constant};

/// Multiprecision modal form of one cutoff-compensated Butterworth pass and
/// its forward-backward kernel.
#[derive(Clone, Debug)]
pub struct ButterworthModes {
    precision: u32,
    order: usize,
    alpha: Float,
    alpha_power: Float,
    poles: Vec<Complex>,
    residues: Vec<Complex>,
    kernel_coefficients: Vec<Complex>,
}

impl ButterworthModes {
    #[must_use]
    pub fn new(order: usize, precision: u32) -> Self {
        assert!(order > 0, "the per-pass order must be positive");
        assert!(
            precision >= 192,
            "the validated oracle requires at least 192 bits"
        );

        let mut sqrt_two = Float::with_val(precision, 2);
        sqrt_two.sqrt_mut();
        sqrt_two -= 1;

        let mut exponent = Float::with_val(precision, -1);
        exponent /= 2 * order;
        let mut alpha = sqrt_two.ln();
        alpha *= exponent;
        alpha.exp_mut();

        let mut alpha_power = Float::with_val(precision, 1);
        for _ in 0..order {
            alpha_power *= &alpha;
        }

        let pi = Float::with_val(precision, Constant::Pi);
        let poles = (0..order)
            .map(|index| {
                let numerator = 2 * index + order + 1;
                let mut angle = Float::with_val(precision, &pi);
                angle *= numerator;
                angle /= 2 * order;
                let (sin, cos) = angle.sin_cos(Float::new(precision));

                let mut real = Float::with_val(precision, &alpha);
                real *= cos;
                let mut imaginary = Float::with_val(precision, &alpha);
                imaginary *= sin;
                Complex::with_val(precision, (real, imaginary))
            })
            .collect::<Vec<_>>();

        let residues = (0..order)
            .map(|index| {
                let mut denominator = Complex::with_val(precision, 1);
                for other in 0..order {
                    if other == index {
                        continue;
                    }
                    let mut difference = poles[index].clone();
                    difference -= &poles[other];
                    denominator *= difference;
                }
                let mut residue = Complex::with_val(precision, &alpha_power);
                residue /= denominator;
                residue
            })
            .collect::<Vec<_>>();

        let kernel_coefficients = (0..order)
            .map(|index| {
                let mut denominator = Complex::with_val(precision, 1);
                for pole in &poles {
                    let mut factor = -poles[index].clone();
                    factor -= pole;
                    denominator *= factor;
                }
                let mut response_at_reflected_pole = Complex::with_val(precision, &alpha_power);
                response_at_reflected_pole /= denominator;

                let mut coefficient = residues[index].clone();
                coefficient *= response_at_reflected_pole;
                coefficient
            })
            .collect();

        Self {
            precision,
            order,
            alpha,
            alpha_power,
            poles,
            residues,
            kernel_coefficients,
        }
    }

    #[must_use]
    pub const fn precision(&self) -> u32 {
        self.precision
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    #[must_use]
    pub const fn alpha(&self) -> &Float {
        &self.alpha
    }

    #[must_use]
    pub fn poles(&self) -> &[Complex] {
        &self.poles
    }

    #[must_use]
    pub fn residues(&self) -> &[Complex] {
        &self.residues
    }

    #[must_use]
    pub fn kernel_coefficients(&self) -> &[Complex] {
        &self.kernel_coefficients
    }

    #[must_use]
    pub fn transfer(&self, argument: &Complex) -> Complex {
        let mut denominator = Complex::with_val(self.precision, 1);
        for pole in &self.poles {
            let mut factor = argument.clone();
            factor -= pole;
            denominator *= factor;
        }
        let mut response = Complex::with_val(self.precision, &self.alpha_power);
        response /= denominator;
        response
    }

    #[must_use]
    pub fn transfer_from_residues(&self, argument: &Complex) -> Complex {
        let mut response = Complex::new(self.precision);
        for (pole, residue) in self.poles.iter().zip(&self.residues) {
            let mut denominator = argument.clone();
            denominator -= pole;
            let mut term = residue.clone();
            term /= denominator;
            response += term;
        }
        response
    }

    #[must_use]
    pub fn kernel_coefficient_from_double_sum(&self, index: usize) -> Complex {
        let mut sum = Complex::new(self.precision);
        for (pole, residue) in self.poles.iter().zip(&self.residues) {
            let mut denominator = pole.clone();
            denominator += &self.poles[index];
            let mut term = residue.clone();
            term /= denominator;
            sum += term;
        }
        let mut coefficient = self.residues[index].clone();
        coefficient *= sum;
        -coefficient
    }

    #[must_use]
    pub fn kernel_right(&self, distance: &Float) -> Complex {
        assert!(*distance >= 0, "kernel distance must be nonnegative");
        let mut value = Complex::new(self.precision);
        for (pole, coefficient) in self.poles.iter().zip(&self.kernel_coefficients) {
            let exponential = if distance == &0 {
                Complex::with_val(self.precision, 1)
            } else {
                let mut argument = pole.clone();
                argument *= distance;
                argument.exp()
            };
            let mut term = coefficient.clone();
            term *= exponential;
            value += term;
        }
        value
    }

    /// Evaluates the algebraically equivalent signed-left double sum. This is
    /// intentionally separate from `kernel_right` so symmetry tests do not
    /// call one shared `abs(distance)` implementation twice.
    #[must_use]
    pub fn kernel_left(&self, distance: &Float) -> Complex {
        assert!(*distance >= 0, "kernel distance must be nonnegative");
        let mut value = Complex::new(self.precision);
        for (left_pole, left_residue) in self.poles.iter().zip(&self.residues) {
            let exponential = if distance == &0 {
                Complex::with_val(self.precision, 1)
            } else {
                let mut argument = left_pole.clone();
                argument *= distance;
                argument.exp()
            };
            for (right_pole, right_residue) in self.poles.iter().zip(&self.residues) {
                let mut denominator = left_pole.clone();
                denominator += right_pole;
                let mut term = left_residue.clone();
                term *= right_residue;
                term *= &exponential;
                term /= denominator;
                value -= term;
            }
        }
        value
    }

    #[must_use]
    pub fn kernel_transform(&self, normalized_frequency: &Float) -> Complex {
        let mut value = Complex::new(self.precision);
        let frequency_squared =
            Float::with_val(self.precision, normalized_frequency * normalized_frequency);
        for (pole, coefficient) in self.poles.iter().zip(&self.kernel_coefficients) {
            let mut denominator = pole.clone();
            denominator *= pole;
            denominator += &frequency_squared;

            let mut numerator = coefficient.clone();
            numerator *= pole;
            numerator *= -2;
            numerator /= denominator;
            value += numerator;
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_error(left: &Complex, right: &Complex) -> f64 {
        let difference = Complex::with_val(left.prec().0, left - right);
        Float::with_val(left.prec().0, difference.abs_ref()).to_f64()
    }

    fn assert_negligible_imaginary(value: &Complex) {
        let precision = value.prec().0;
        let mut scale = value.real().clone().abs();
        if scale < 1 {
            scale = Float::with_val(precision, 1);
        }
        let mut limit = Float::with_val(precision, 1.0e-30);
        limit *= scale;
        assert!(value.imag().clone().abs() <= limit);
    }

    #[test]
    fn first_order_kernel_has_the_closed_form() {
        let modes = ButterworthModes::new(1, 192);
        let distance = Float::with_val(192, 0.75);
        let actual = modes.kernel_right(&distance);

        let mut expected_real = Float::with_val(192, modes.alpha());
        expected_real /= 2;
        let mut exponent = Float::with_val(192, modes.alpha());
        exponent *= &distance;
        exponent = -exponent;
        exponent.exp_mut();
        expected_real *= exponent;
        let expected = Complex::with_val(192, expected_real);

        assert!(absolute_error(&actual, &expected) < 1.0e-50);
    }

    #[test]
    fn residues_reconstruct_the_transfer_function() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 192);
            for frequency in [0.0, 0.0625, 0.25, 0.7, 1.0, 2.0, 4.0] {
                let argument = Complex::with_val(192, (0, frequency));
                let product = modes.transfer(&argument);
                let partial_fractions = modes.transfer_from_residues(&argument);
                assert!(absolute_error(&product, &partial_fractions) < 1.0e-48);
            }
        }
    }

    #[test]
    fn product_and_double_sum_kernel_coefficients_agree() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 256);
            for index in 0..order {
                let double_sum = modes.kernel_coefficient_from_double_sum(index);
                assert!(absolute_error(&modes.kernel_coefficients[index], &double_sum) < 1.0e-65);
            }
        }
    }

    #[test]
    fn independent_signed_kernel_branches_are_symmetric() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 192);
            for distance in [0.0, 0.001, 0.5, 3.0] {
                let distance = Float::with_val(192, distance);
                let right = modes.kernel_right(&distance);
                let left = modes.kernel_left(&distance);
                assert!(absolute_error(&right, &left) < 1.0e-48);
                assert_negligible_imaginary(&right);
                assert_negligible_imaginary(&left);
            }
        }
    }

    #[test]
    fn kernel_transform_matches_the_squared_butterworth_gain() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 256);
            for frequency in [0.0, 0.0625, 0.25, 0.7, 1.0, 2.0, 4.0] {
                let frequency = Float::with_val(256, frequency);
                let actual = modes.kernel_transform(&frequency);

                let mut ratio = Float::with_val(256, &frequency);
                ratio /= modes.alpha();
                let mut power = Float::with_val(256, 1);
                for _ in 0..(2 * order) {
                    power *= &ratio;
                }
                power += 1;
                power.recip_mut();
                let expected = Complex::with_val(256, power);

                assert!(absolute_error(&actual, &expected) < 1.0e-64);
            }
        }
    }

    #[test]
    fn modes_are_stable_and_form_conjugate_pairs() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 192);
            for index in 0..order {
                let conjugate_index = order - 1 - index;
                let pole = &modes.poles[index];
                assert!(pole.real() < &0);

                let magnitude = Float::with_val(192, pole.abs_ref());
                let radius_error = Float::with_val(192, magnitude - &modes.alpha).abs();
                assert!(radius_error < Float::with_val(192, 1.0e-50));

                let conjugate_pole = modes.poles[index].clone().conj();
                assert!(absolute_error(&conjugate_pole, &modes.poles[conjugate_index]) < 1.0e-50);
                let conjugate_residue = modes.residues[index].clone().conj();
                assert!(
                    absolute_error(&conjugate_residue, &modes.residues[conjugate_index]) < 1.0e-49
                );
                let conjugate_coefficient = modes.kernel_coefficients[index].clone().conj();
                assert!(
                    absolute_error(
                        &conjugate_coefficient,
                        &modes.kernel_coefficients[conjugate_index]
                    ) < 1.0e-49
                );
            }
        }
    }

    #[test]
    fn public_frequency_one_is_minus_three_db_after_two_passes() {
        for order in [1, 2, 4, 8] {
            let modes = ButterworthModes::new(order, 256);
            let actual = modes.kernel_transform(&Float::with_val(256, 1));
            let mut expected = Float::with_val(256, 2);
            expected.sqrt_mut();
            expected.recip_mut();
            let error = Float::with_val(256, actual.real() - expected).abs();
            assert!(error < Float::with_val(256, 1.0e-64));
            assert!(actual.imag().clone().abs() < Float::with_val(256, 1.0e-64));
        }
    }
}
