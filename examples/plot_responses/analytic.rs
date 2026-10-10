use std::error::Error;

use num_traits::ToPrimitive;
use ssfilt::Response;

#[path = "../../src/model/bessel_table.rs"]
mod bessel_table;

const FREQUENCY_POINTS: u32 = 1_601;
pub(super) const MIN_FREQUENCY_RATIO: f64 = 0.01;
pub(super) const MAX_FREQUENCY_RATIO: f64 = 20.0;
pub(super) const MAGNITUDE_FLOOR_DB: f64 = -140.0;
const GROUP_DELAY_HALF_WINDOW: usize = 4;
const SETTLING_E_FOLDS: f64 = 24.0;

pub(super) struct FrequencyPoint {
    pub(super) ratio: f64,
    pub(super) gain_db: f64,
    pub(super) phase_radians: f64,
}

pub(super) fn frequency_ratios() -> impl Iterator<Item = f64> {
    let start = MIN_FREQUENCY_RATIO.log10();
    let end = MAX_FREQUENCY_RATIO.log10();
    (0..FREQUENCY_POINTS).map(move |index| {
        let fraction = f64::from(index) / f64::from(FREQUENCY_POINTS - 1);
        10.0_f64.powf(start + fraction * (end - start))
    })
}

pub(super) fn visible_magnitude_points(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let mut visible = Vec::with_capacity(points.len());
    for point in points {
        if point.gain_db >= MAGNITUDE_FLOOR_DB {
            visible.push((point.ratio, point.gain_db));
            continue;
        }
        if let Some(&(preceding_ratio, preceding_gain)) = visible.last() {
            let fraction = (MAGNITUDE_FLOOR_DB - preceding_gain) / (point.gain_db - preceding_gain);
            let log_ratio = fraction.mul_add(
                point.ratio.ln() - preceding_ratio.ln(),
                preceding_ratio.ln(),
            );
            visible.push((log_ratio.exp(), MAGNITUDE_FLOOR_DB));
        }
        break;
    }
    visible
}

pub(super) fn exact_frequency_point<const N: usize>(
    response: Response,
    frequency_ratio: f64,
) -> Result<FrequencyPoint, Box<dyn Error>> {
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let mut log_magnitude = 0.0;
    let mut phase_radians = 0.0;
    match response {
        Response::RepeatedPole => {
            let rate = 1.0 / (core::f64::consts::LN_2 / order).exp_m1().sqrt();
            for _ in 0..N {
                accumulate_first_order(
                    rate,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Butterworth => {
            if N % 2 == 1 {
                accumulate_first_order(
                    1.0,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for index in 0..N / 2 {
                let angle =
                    core::f64::consts::PI * (2 * index + 1).to_f64().unwrap() / (2.0 * order);
                accumulate_second_order(
                    2.0 * angle.sin(),
                    1.0,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Bessel => {
            let prototype = bessel_table::prototype(N)
                .ok_or("Bessel response order exceeds the supported table")?;
            if N % 2 == 1 {
                accumulate_first_order(
                    prototype.real_rate,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for (&damping, &frequency_squared) in
                prototype.damping.iter().zip(prototype.frequency_squared)
            {
                accumulate_second_order(
                    damping,
                    frequency_squared,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        Response::Chebyshev1 { ripple_db } => {
            let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
            let epsilon = epsilon_squared.sqrt();
            let mu = (1.0 / epsilon).asinh() / order;
            let cutoff_target = if N % 2 == 0 {
                (1.0 / epsilon_squared + 2.0).sqrt()
            } else {
                1.0 / epsilon
            };
            let cutoff_scale = (cutoff_target.acosh() / order).cosh();
            if N % 2 == 1 {
                accumulate_first_order(
                    mu.sinh() / cutoff_scale,
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
            for index in 0..N / 2 {
                let angle =
                    core::f64::consts::PI * (2 * index + 1).to_f64().unwrap() / (2.0 * order);
                let real = mu.sinh() * angle.sin() / cutoff_scale;
                let imaginary = mu.cosh() * angle.cos() / cutoff_scale;
                accumulate_second_order(
                    2.0 * real,
                    real.mul_add(real, imaginary * imaginary),
                    frequency_ratio,
                    &mut log_magnitude,
                    &mut phase_radians,
                );
            }
        }
        _ => return Err("response is not supported by this development plot".into()),
    }

    Ok(FrequencyPoint {
        ratio: frequency_ratio,
        gain_db: 20.0 / core::f64::consts::LN_10 * log_magnitude,
        phase_radians,
    })
}

/// Returns a conservative warm-up time for a one-hertz-cutoff filter.
///
/// The bound gives the slowest pole 24 exponential time constants. The tone
/// measurement also enforces a 60-input-cycle minimum, which dominates the
/// repeated-pole multiplicity and ordinary low-order cases.
pub(super) fn minimum_settling_time_seconds<const N: usize>(
    response: Response,
) -> Result<f64, Box<dyn Error>> {
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let slowest_normalized_decay = match response {
        Response::RepeatedPole => 1.0 / (core::f64::consts::LN_2 / order).exp_m1().sqrt(),
        Response::Butterworth => {
            if N == 1 {
                1.0
            } else {
                (core::f64::consts::PI / (2.0 * order)).sin()
            }
        }
        Response::Bessel => {
            let prototype = bessel_table::prototype(N)
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
            let epsilon_squared = (core::f64::consts::LN_10 * ripple_db / 10.0).exp_m1();
            let epsilon = epsilon_squared.sqrt();
            let mu = (1.0 / epsilon).asinh() / order;
            let cutoff_target = if N % 2 == 0 {
                (1.0 / epsilon_squared + 2.0).sqrt()
            } else {
                1.0 / epsilon
            };
            let cutoff_scale = (cutoff_target.acosh() / order).cosh();
            if N == 1 {
                mu.sinh() / cutoff_scale
            } else {
                mu.sinh() * (core::f64::consts::PI / (2.0 * order)).sin() / cutoff_scale
            }
        }
        _ => return Err("response is not supported by this development plot".into()),
    };
    if !slowest_normalized_decay.is_finite() || slowest_normalized_decay <= 0.0 {
        return Err("response has no finite stable settling rate".into());
    }
    Ok(SETTLING_E_FOLDS / (core::f64::consts::TAU * slowest_normalized_decay))
}

fn accumulate_first_order(rate: f64, frequency: f64, log_magnitude: &mut f64, phase: &mut f64) {
    *log_magnitude += rate.ln() - rate.hypot(frequency).ln();
    *phase -= frequency.atan2(rate);
}

fn accumulate_second_order(
    damping: f64,
    natural_frequency_squared: f64,
    frequency: f64,
    log_magnitude: &mut f64,
    phase: &mut f64,
) {
    let denominator_real = natural_frequency_squared - frequency * frequency;
    let denominator_imaginary = damping * frequency;
    *log_magnitude +=
        natural_frequency_squared.ln() - denominator_real.hypot(denominator_imaginary).ln();
    *phase -= denominator_imaginary.atan2(denominator_real);
}

pub(super) fn group_delay(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let width = 2 * GROUP_DELAY_HALF_WINDOW + 1;
    points
        .windows(width)
        .filter_map(|window| {
            let center = &window[GROUP_DELAY_HALF_WINDOW];
            let mean_ratio = window.iter().map(|point| point.ratio).sum::<f64>()
                / width.to_f64().expect("small constant fits in f64");
            let mean_phase = window.iter().map(|point| point.phase_radians).sum::<f64>()
                / width.to_f64().expect("small constant fits in f64");
            let (covariance, variance) = window.iter().fold((0.0, 0.0), |acc, point| {
                let ratio_offset = point.ratio - mean_ratio;
                (
                    (point.phase_radians - mean_phase).mul_add(ratio_offset, acc.0),
                    ratio_offset.mul_add(ratio_offset, acc.1),
                )
            });
            let delay = -covariance / variance;
            (delay.is_finite() && delay >= 0.0).then_some((center.ratio, delay))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::{
        MAGNITUDE_FLOOR_DB, exact_frequency_point, frequency_ratios, minimum_settling_time_seconds,
        visible_magnitude_points,
    };
    use ssfilt::Response;

    #[test]
    fn frequency_grid_is_logarithmically_spaced() {
        let ratios = frequency_ratios().collect::<Vec<_>>();
        let expected_ratio = ratios[1] / ratios[0];
        for pair in ratios.windows(2) {
            assert_relative_eq!(pair[1] / pair[0], expected_ratio, epsilon = 2.0e-15);
        }
    }

    #[test]
    fn exact_responses_share_the_minus_three_db_cutoff() {
        for response in [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let point = exact_frequency_point::<36>(response, 1.0).unwrap();
            assert_relative_eq!(point.gain_db, -3.010_299_956_639_812, epsilon = 2.0e-11);
            assert!(point.phase_radians.is_finite());
        }

        let bessel = exact_frequency_point::<25>(Response::Bessel, 1.0).unwrap();
        assert_relative_eq!(bessel.gain_db, -3.010_299_956_639_812, epsilon = 2.0e-11);
        assert!(bessel.phase_radians.is_finite());
    }

    #[test]
    fn exact_high_order_stopbands_reach_the_plot_floor() {
        for response in [
            Response::Butterworth,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ] {
            let point = exact_frequency_point::<36>(response, 20.0).unwrap();
            assert!(point.gain_db < MAGNITUDE_FLOOR_DB);
        }
    }

    #[test]
    fn magnitude_curve_ends_at_the_visible_floor() {
        let points = frequency_ratios()
            .map(|ratio| {
                exact_frequency_point::<36>(Response::Chebyshev1 { ripple_db: 0.5 }, ratio)
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let visible = visible_magnitude_points(&points);
        assert_eq!(visible.last().unwrap().1, MAGNITUDE_FLOOR_DB);
        assert!(visible.len() < points.len());
    }

    #[test]
    fn high_order_chebyshev_receives_a_longer_settling_window() {
        let low_order =
            minimum_settling_time_seconds::<4>(Response::Chebyshev1 { ripple_db: 0.5 }).unwrap();
        let high_order =
            minimum_settling_time_seconds::<50>(Response::Chebyshev1 { ripple_db: 0.5 }).unwrap();
        assert!(low_order > 0.0);
        assert!(high_order > 3_000.0);
        assert!(high_order > 100.0 * low_order);
    }
}
