use std::error::Error;

use ssfilt::Response;

#[path = "../support/transfer.rs"]
mod transfer;

const FREQUENCY_POINTS: u32 = 1_601;
pub(super) const MIN_FREQUENCY_RATIO: f64 = 0.01;
pub(super) const MAX_FREQUENCY_RATIO: f64 = 20.0;
pub(super) const MAGNITUDE_FLOOR_DB: f64 = -140.0;
const SETTLING_E_FOLDS: f64 = 24.0;

pub(super) struct FrequencyPoint {
    pub(super) ratio: f64,
    pub(super) gain_db: f64,
    pub(super) phase_radians: f64,
    pub(super) normalized_group_delay: f64,
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
) -> Result<FrequencyPoint, Box<dyn Error + Send + Sync>> {
    let point = transfer::low_pass(response, N, frequency_ratio)?;

    Ok(FrequencyPoint {
        ratio: frequency_ratio,
        gain_db: point.gain_db,
        phase_radians: point.phase_radians,
        normalized_group_delay: point.normalized_group_delay,
    })
}

/// Returns a conservative warm-up time for a one-hertz-cutoff filter.
///
/// The bound gives the slowest pole 24 exponential time constants. The tone
/// measurement also enforces a 60-input-cycle minimum, which dominates the
/// repeated-pole multiplicity and ordinary low-order cases.
pub(super) fn minimum_settling_time_seconds<const N: usize>(
    response: Response,
) -> Result<f64, Box<dyn Error + Send + Sync>> {
    let slowest_normalized_decay = transfer::slowest_normalized_decay(response, N)?;
    Ok(SETTLING_E_FOLDS / (core::f64::consts::TAU * slowest_normalized_decay))
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
    fn frequency_points_keep_the_oracles_exact_group_delay() {
        let point = exact_frequency_point::<1>(Response::Butterworth, 0.75).unwrap();
        assert_relative_eq!(
            point.normalized_group_delay,
            1.0 / 0.75_f64.mul_add(0.75, 1.0),
            epsilon = 1.0e-14
        );
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
