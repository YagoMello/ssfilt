//! Independent normalized low-pass transfer-function evaluation for examples.
//!
//! This module intentionally evaluates pole contributions directly. It is a
//! development oracle, not part of the runtime implementation.

use std::error::Error;

use num_traits::ToPrimitive;
use ssfilt::Response;

#[path = "../../src/model/bessel_table.rs"]
mod bessel_table;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransferPoint {
    pub(crate) gain_db: f64,
    pub(crate) phase_radians: f64,
    pub(crate) normalized_group_delay: f64,
}

pub(crate) fn low_pass(
    response: Response,
    order: usize,
    frequency_ratio: f64,
) -> Result<TransferPoint, Box<dyn Error>> {
    if order == 0 {
        return Err("transfer-function order must be positive".into());
    }
    if !frequency_ratio.is_finite() || frequency_ratio < 0.0 {
        return Err("frequency ratio must be finite and nonnegative".into());
    }

    let order_float = order.to_f64().ok_or("order does not fit in f64")?;
    let mut point = TransferPoint {
        gain_db: 0.0,
        phase_radians: 0.0,
        normalized_group_delay: 0.0,
    };
    match response {
        Response::RepeatedPole => {
            let rate = 1.0 / (core::f64::consts::LN_2 / order_float).exp_m1().sqrt();
            for _ in 0..order {
                accumulate_first_order(rate, frequency_ratio, &mut point);
            }
        }
        Response::Butterworth => {
            if order % 2 == 1 {
                accumulate_first_order(1.0, frequency_ratio, &mut point);
            }
            for index in 0..order / 2 {
                let angle = core::f64::consts::PI
                    * (2 * index + 1)
                        .to_f64()
                        .ok_or("section index does not fit in f64")?
                    / (2.0 * order_float);
                accumulate_second_order(2.0 * angle.sin(), 1.0, frequency_ratio, &mut point);
            }
        }
        Response::Bessel => {
            let prototype = bessel_table::prototype(order)
                .ok_or("Bessel response order exceeds the supported table")?;
            if order % 2 == 1 {
                accumulate_first_order(prototype.real_rate, frequency_ratio, &mut point);
            }
            for (&damping, &frequency_squared) in
                prototype.damping.iter().zip(prototype.frequency_squared)
            {
                accumulate_second_order(damping, frequency_squared, frequency_ratio, &mut point);
            }
        }
        Response::Chebyshev1 { ripple_db } => {
            if !ripple_db.is_finite()
                || ripple_db <= 0.0
                || ripple_db >= 10.0 * core::f64::consts::LOG10_2
            {
                return Err(
                    "Chebyshev ripple must be finite and between zero and 3.0103 dB".into(),
                );
            }
            let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
            let epsilon = epsilon_squared.sqrt();
            let mu = (1.0 / epsilon).asinh() / order_float;
            let cutoff_target = if order % 2 == 0 {
                (1.0 / epsilon_squared + 2.0).sqrt()
            } else {
                1.0 / epsilon
            };
            let cutoff_scale = (cutoff_target.acosh() / order_float).cosh();
            if order % 2 == 1 {
                accumulate_first_order(mu.sinh() / cutoff_scale, frequency_ratio, &mut point);
            }
            for index in 0..order / 2 {
                let angle = core::f64::consts::PI
                    * (2 * index + 1)
                        .to_f64()
                        .ok_or("section index does not fit in f64")?
                    / (2.0 * order_float);
                let real = mu.sinh() * angle.sin() / cutoff_scale;
                let imaginary = mu.cosh() * angle.cos() / cutoff_scale;
                accumulate_second_order(
                    2.0 * real,
                    real.mul_add(real, imaginary * imaginary),
                    frequency_ratio,
                    &mut point,
                );
            }
        }
        _ => return Err("response is not supported by the analytic low-pass oracle".into()),
    }
    point.gain_db *= 20.0 / core::f64::consts::LN_10;
    if !point.gain_db.is_finite()
        || !point.phase_radians.is_finite()
        || !point.normalized_group_delay.is_finite()
        || point.normalized_group_delay < 0.0
    {
        return Err("transfer-function evaluation is not finite".into());
    }
    Ok(point)
}

#[allow(dead_code)] // Shared consumers do not all need settling-time data.
pub(crate) fn slowest_normalized_decay(
    response: Response,
    order: usize,
) -> Result<f64, Box<dyn Error>> {
    if order == 0 {
        return Err("transfer-function order must be positive".into());
    }
    let order_float = order.to_f64().ok_or("order does not fit in f64")?;
    let decay = match response {
        Response::RepeatedPole => 1.0 / (core::f64::consts::LN_2 / order_float).exp_m1().sqrt(),
        Response::Butterworth => {
            if order == 1 {
                1.0
            } else {
                (core::f64::consts::PI / (2.0 * order_float)).sin()
            }
        }
        Response::Bessel => {
            let prototype = bessel_table::prototype(order)
                .ok_or("Bessel response order exceeds the supported table")?;
            let section_decay = prototype
                .damping
                .iter()
                .map(|damping| damping / 2.0)
                .fold(f64::INFINITY, f64::min);
            if prototype.real_rate > 0.0 {
                prototype.real_rate.min(section_decay)
            } else {
                section_decay
            }
        }
        Response::Chebyshev1 { ripple_db } => {
            if !ripple_db.is_finite()
                || ripple_db <= 0.0
                || ripple_db >= 10.0 * core::f64::consts::LOG10_2
            {
                return Err(
                    "Chebyshev ripple must be finite and between zero and 3.0103 dB".into(),
                );
            }
            let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
            let epsilon = epsilon_squared.sqrt();
            let mu = (1.0 / epsilon).asinh() / order_float;
            let cutoff_target = if order % 2 == 0 {
                (1.0 / epsilon_squared + 2.0).sqrt()
            } else {
                1.0 / epsilon
            };
            let cutoff_scale = (cutoff_target.acosh() / order_float).cosh();
            if order == 1 {
                mu.sinh() / cutoff_scale
            } else {
                mu.sinh() * (core::f64::consts::PI / (2.0 * order_float)).sin() / cutoff_scale
            }
        }
        _ => return Err("response is not supported by the analytic low-pass oracle".into()),
    };
    if !decay.is_finite() || decay <= 0.0 {
        return Err("response has no finite stable settling rate".into());
    }
    Ok(decay)
}

fn accumulate_first_order(rate: f64, frequency: f64, point: &mut TransferPoint) {
    point.gain_db += rate.ln() - rate.hypot(frequency).ln();
    point.phase_radians -= frequency.atan2(rate);
    point.normalized_group_delay += rate / rate.mul_add(rate, frequency * frequency);
}

fn accumulate_second_order(
    damping: f64,
    natural_frequency_squared: f64,
    frequency: f64,
    point: &mut TransferPoint,
) {
    let frequency_squared = frequency * frequency;
    let denominator_real = natural_frequency_squared - frequency_squared;
    let denominator_imaginary = damping * frequency;
    let denominator_power = denominator_real.mul_add(
        denominator_real,
        denominator_imaginary * denominator_imaginary,
    );
    point.gain_db += natural_frequency_squared.ln() - 0.5 * denominator_power.ln();
    point.phase_radians -= denominator_imaginary.atan2(denominator_real);
    point.normalized_group_delay +=
        damping * (natural_frequency_squared + frequency_squared) / denominator_power;
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use ssfilt::Response;

    use super::low_pass;

    #[test]
    fn response_families_share_the_whole_filter_minus_three_db_cutoff() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            for order in [1, 2, 4, 8] {
                let point = low_pass(response, order, 1.0).unwrap();
                assert_relative_eq!(
                    point.gain_db,
                    -10.0 * core::f64::consts::LOG10_2,
                    epsilon = 3.0e-12
                );
            }
        }
    }

    #[test]
    fn exact_group_delay_matches_a_phase_derivative_cross_check() {
        let frequency = 0.73;
        let step = 1.0e-6;
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let point = low_pass(response, 8, frequency).unwrap();
            let before = low_pass(response, 8, frequency - step).unwrap();
            let after = low_pass(response, 8, frequency + step).unwrap();
            let finite_difference = -(after.phase_radians - before.phase_radians) / (2.0 * step);
            assert_relative_eq!(
                point.normalized_group_delay,
                finite_difference,
                epsilon = 2.0e-8
            );
        }
    }

    #[test]
    fn first_order_butterworth_has_known_delay() {
        for frequency in [0.0, 0.25, 1.0, 4.0] {
            let point = low_pass(Response::Butterworth, 1, frequency).unwrap();
            assert_relative_eq!(
                point.normalized_group_delay,
                1.0 / frequency.mul_add(frequency, 1.0),
                epsilon = 1.0e-14
            );
        }
    }

    #[test]
    fn non_finite_derived_values_are_rejected() {
        assert!(
            low_pass(
                Response::Chebyshev1 {
                    ripple_db: f64::from_bits(1),
                },
                4,
                1.0,
            )
            .is_err()
        );
        assert!(low_pass(Response::Butterworth, 4, f64::MAX).is_err());
    }
}
