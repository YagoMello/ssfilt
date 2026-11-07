use anyhow::{Result, bail};
use num_traits::ToPrimitive;
use ssfilt::Response;

use crate::{Topology, transfer};

const FREQUENCY_POINTS: u32 = 1_601;
pub(super) const MAGNITUDE_FLOOR_DB: f64 = -120.0;
const SETTLING_E_FOLDS: f64 = 24.0;
const BAND_FRACTIONAL_BANDWIDTH: f64 = 1.5;

#[derive(Clone, Copy, Debug)]
pub(super) struct FrequencyPoint {
    pub(super) ratio: f64,
    pub(super) gain_db: f64,
    pub(super) phase_radians: Option<f64>,
    pub(super) normalized_group_delay: Option<f64>,
}

pub(super) fn frequency_range(topology: Topology) -> (f64, f64) {
    match topology {
        Topology::LowPass | Topology::HighPass => (0.01, 20.0),
        Topology::BandPass | Topology::BandStop => (0.05, 20.0),
    }
}

pub(super) fn frequency_ratios(topology: Topology) -> impl Iterator<Item = f64> {
    let (minimum, maximum) = frequency_range(topology);
    let start = minimum.log10();
    let end = maximum.log10();
    (0..FREQUENCY_POINTS).map(move |index| {
        if index == 0 {
            return minimum;
        }
        if index == FREQUENCY_POINTS - 1 {
            return maximum;
        }
        let fraction = f64::from(index) / f64::from(FREQUENCY_POINTS - 1);
        10.0_f64.powf(start + fraction * (end - start))
    })
}

pub(super) fn frequency_point(
    topology: Topology,
    response: Response,
    final_order: usize,
    ratio: f64,
) -> Result<FrequencyPoint> {
    if !ratio.is_finite() || ratio <= 0.0 {
        bail!("normalized frequency must be finite and positive");
    }
    let prototype_order = topology.prototype_order(final_order);
    let point = match topology {
        Topology::LowPass => transfer::low_pass(response, prototype_order, ratio)?,
        Topology::HighPass => {
            let low_pass = transfer::low_pass(response, prototype_order, 1.0 / ratio)?;
            return Ok(FrequencyPoint {
                ratio,
                gain_db: low_pass.gain_db,
                phase_radians: Some(-low_pass.phase_radians),
                normalized_group_delay: Some(low_pass.normalized_group_delay / (ratio * ratio)),
            });
        }
        Topology::BandPass => {
            if final_order == 0 || !final_order.is_multiple_of(2) {
                bail!("band-pass final order must be positive and even");
            }
            let mapped = (ratio - 1.0 / ratio) / BAND_FRACTIONAL_BANDWIDTH;
            let low_pass = transfer::low_pass(response, prototype_order, mapped.abs())?;
            return Ok(FrequencyPoint {
                ratio,
                gain_db: low_pass.gain_db,
                phase_radians: Some(low_pass.phase_radians * mapped.signum()),
                normalized_group_delay: Some(
                    low_pass.normalized_group_delay * (1.0 + 1.0 / (ratio * ratio))
                        / BAND_FRACTIONAL_BANDWIDTH,
                ),
            });
        }
        Topology::BandStop => {
            if final_order == 0 || !final_order.is_multiple_of(2) {
                bail!("band-stop final order must be positive and even");
            }
            if ratio.to_bits() == 1.0_f64.to_bits() {
                return Ok(FrequencyPoint {
                    ratio,
                    gain_db: f64::NEG_INFINITY,
                    phase_radians: None,
                    normalized_group_delay: None,
                });
            }
            let denominator = 1.0 - ratio * ratio;
            let mapped = BAND_FRACTIONAL_BANDWIDTH * ratio / denominator;
            let low_pass = transfer::low_pass(response, prototype_order, mapped.abs())?;
            return Ok(FrequencyPoint {
                ratio,
                gain_db: low_pass.gain_db,
                phase_radians: Some(low_pass.phase_radians * mapped.signum()),
                normalized_group_delay: Some(
                    low_pass.normalized_group_delay
                        * BAND_FRACTIONAL_BANDWIDTH
                        * (1.0 + ratio * ratio)
                        / (denominator * denominator),
                ),
            });
        }
    };
    Ok(FrequencyPoint {
        ratio,
        gain_db: point.gain_db,
        phase_radians: Some(point.phase_radians),
        normalized_group_delay: Some(point.normalized_group_delay),
    })
}

pub(super) fn minimum_settling_time_seconds(
    topology: Topology,
    response: Response,
    final_order: usize,
) -> Result<f64> {
    let prototype_order = topology.prototype_order(final_order);
    let decay = transfer::slowest_normalized_decay(response, prototype_order)?;
    let topology_scale = if matches!(topology, Topology::BandPass | Topology::BandStop) {
        2.0
    } else {
        1.0
    };
    let settling = topology_scale * SETTLING_E_FOLDS / (core::f64::consts::TAU * decay);
    if !settling.is_finite() || settling <= 0.0 {
        bail!("response has no finite positive settling window");
    }
    Ok(settling)
}

pub(super) fn visible_magnitude_points(points: &[FrequencyPoint]) -> Vec<(f64, f64)> {
    let mut visible = Vec::with_capacity(points.len());
    for pair in points.windows(2) {
        let preceding = &pair[0];
        let current = &pair[1];
        if visible.is_empty() && preceding.gain_db >= MAGNITUDE_FLOOR_DB {
            visible.push((preceding.ratio, preceding.gain_db));
        }
        let preceding_visible = preceding.gain_db >= MAGNITUDE_FLOOR_DB;
        let current_visible = current.gain_db >= MAGNITUDE_FLOOR_DB;
        if preceding_visible != current_visible {
            let crossing_ratio = if preceding.gain_db.is_finite() && current.gain_db.is_finite() {
                let fraction = (MAGNITUDE_FLOOR_DB - preceding.gain_db)
                    / (current.gain_db - preceding.gain_db);
                fraction
                    .mul_add(
                        current.ratio.ln() - preceding.ratio.ln(),
                        preceding.ratio.ln(),
                    )
                    .exp()
            } else if preceding_visible {
                current.ratio
            } else {
                preceding.ratio
            };
            visible.push((crossing_ratio, MAGNITUDE_FLOOR_DB));
        }
        if current_visible {
            visible.push((current.ratio, current.gain_db));
        }
    }
    if points.len() == 1 && points[0].gain_db >= MAGNITUDE_FLOOR_DB {
        visible.push((points[0].ratio, points[0].gain_db));
    }
    visible
}

pub(super) fn phase_near(wrapped: f64, reference: f64) -> f64 {
    wrapped + core::f64::consts::TAU * ((reference - wrapped) / core::f64::consts::TAU).round()
}

pub(super) fn usize_as_f64(value: usize) -> Result<f64> {
    value
        .to_f64()
        .ok_or_else(|| anyhow::anyhow!("value does not fit in f64"))
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use ssfilt::Response;

    use super::{
        frequency_point, frequency_range, frequency_ratios, phase_near, visible_magnitude_points,
    };
    use crate::{Topology, transfer};

    #[test]
    fn log_frequency_grids_are_monotonic_and_cover_the_view_range() {
        for topology in [
            Topology::LowPass,
            Topology::HighPass,
            Topology::BandPass,
            Topology::BandStop,
        ] {
            let ratios = frequency_ratios(topology).collect::<Vec<_>>();
            assert_eq!(ratios.len(), 1_601);
            assert_eq!(ratios.first().copied(), Some(frequency_range(topology).0));
            assert_eq!(ratios.last().copied(), Some(frequency_range(topology).1));
            assert!(ratios.windows(2).all(|pair| pair[0] < pair[1]));
            if topology == Topology::BandStop {
                assert!(
                    ratios
                        .iter()
                        .any(|ratio| ratio.to_bits() == 1.0_f64.to_bits())
                );
            }
        }
    }

    #[test]
    fn all_topologies_use_whole_filter_minus_three_db_cutoffs() {
        let responses = [
            Response::RepeatedPole,
            Response::Butterworth,
            Response::Bessel,
            Response::Chebyshev1 { ripple_db: 0.5 },
        ];
        for response in responses {
            for order in [1, 2, 4, 8] {
                let low = frequency_point(Topology::LowPass, response, order, 1.0).unwrap();
                let high = frequency_point(Topology::HighPass, response, order, 1.0).unwrap();
                assert_relative_eq!(low.gain_db, -3.010_299_956_639_812, epsilon = 3.0e-11);
                assert_relative_eq!(high.gain_db, -3.010_299_956_639_812, epsilon = 3.0e-11);
            }
            for final_order in [2, 4, 8] {
                for edge in [0.5, 2.0] {
                    for topology in [Topology::BandPass, Topology::BandStop] {
                        let point = frequency_point(topology, response, final_order, edge).unwrap();
                        assert_relative_eq!(
                            point.gain_db,
                            -3.010_299_956_639_812,
                            epsilon = 3.0e-11
                        );
                    }
                }
            }
        }
        for edge in [0.5, 2.0] {
            for topology in [Topology::BandPass, Topology::BandStop] {
                assert_relative_eq!(
                    frequency_point(topology, Response::Bessel, 50, edge)
                        .unwrap()
                        .gain_db,
                    -3.010_299_956_639_812,
                    epsilon = 5.0e-11
                );
            }
        }
    }

    #[test]
    fn topology_group_delay_matches_a_phase_slope_check() {
        let step = 1.0e-6;
        for (topology, order, ratio) in [
            (Topology::LowPass, 8, 0.73),
            (Topology::HighPass, 8, 0.73),
            (Topology::BandPass, 8, 0.8),
            (Topology::BandPass, 8, 1.3),
            (Topology::BandStop, 8, 0.8),
            (Topology::BandStop, 8, 1.3),
        ] {
            let point = frequency_point(topology, Response::Butterworth, order, ratio).unwrap();
            let before =
                frequency_point(topology, Response::Butterworth, order, ratio - step).unwrap();
            let after =
                frequency_point(topology, Response::Butterworth, order, ratio + step).unwrap();
            let derivative =
                -(after.phase_radians.unwrap() - before.phase_radians.unwrap()) / (2.0 * step);
            assert_relative_eq!(
                point.normalized_group_delay.unwrap(),
                derivative,
                epsilon = 2.0e-7
            );
        }
    }

    #[test]
    fn band_stop_center_has_zero_gain_and_undefined_phase() {
        let point = frequency_point(Topology::BandStop, Response::Butterworth, 4, 1.0).unwrap();
        assert_eq!(point.gain_db, f64::NEG_INFINITY);
        assert_eq!(point.phase_radians, None);
        assert_eq!(point.normalized_group_delay, None);
    }

    #[test]
    fn clipped_magnitude_curves_keep_visible_crossings_on_both_sides() {
        let frequencies = [0.1, 0.5, 1.0, 2.0, 10.0];
        let points = frequencies
            .iter()
            .enumerate()
            .map(|(index, ratio)| super::FrequencyPoint {
                ratio: *ratio,
                gain_db: [-140.0, -3.0, 0.0, -3.0, -140.0][index],
                phase_radians: Some(0.0),
                normalized_group_delay: Some(0.0),
            })
            .collect::<Vec<_>>();
        let visible = visible_magnitude_points(&points);
        assert_eq!(visible.first().unwrap().1, -120.0);
        assert!((0.1..0.5).contains(&visible.first().unwrap().0));
        assert_eq!(visible.last().unwrap().1, -120.0);
        assert!(visible.len() < points.len() + 2);
    }

    #[test]
    fn clipped_magnitude_curve_handles_an_ideal_zero_without_non_finite_coordinates() {
        let points = [
            super::FrequencyPoint {
                ratio: 0.9,
                gain_db: -40.0,
                phase_radians: Some(-1.0),
                normalized_group_delay: Some(1.0),
            },
            super::FrequencyPoint {
                ratio: 1.0,
                gain_db: f64::NEG_INFINITY,
                phase_radians: None,
                normalized_group_delay: None,
            },
            super::FrequencyPoint {
                ratio: 1.1,
                gain_db: -40.0,
                phase_radians: Some(1.0),
                normalized_group_delay: Some(1.0),
            },
        ];
        let visible = visible_magnitude_points(&points);
        assert_eq!(visible, vec![(0.9, -40.0), (1.0, -120.0), (1.1, -40.0)]);
        assert!(
            visible
                .iter()
                .all(|(ratio, gain)| ratio.is_finite() && gain.is_finite())
        );
    }

    #[test]
    fn low_pass_phase_branch_and_settling_rate_are_finite() {
        let point = transfer::low_pass(Response::Butterworth, 8, 1.0).unwrap();
        assert!(point.phase_radians < 0.0);
        assert!(
            super::minimum_settling_time_seconds(Topology::BandPass, Response::Bessel, 50)
                .unwrap()
                .is_finite()
        );
        assert_relative_eq!(phase_near(3.0, 3.0), 3.0, epsilon = 0.0);
    }
}
