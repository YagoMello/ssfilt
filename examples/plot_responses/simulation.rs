//! Unit-step traces from the public streaming API.

use anyhow::Result;
use ssfilt::{InputModel, Response};

use crate::Topology;
use crate::analytic::usize_as_f64;
use crate::measure::build_filter;

const TRANSIENT_POINTS: u32 = 1_601;

pub(super) fn measure_step_response<const N: usize>(
    topology: Topology,
    response: Response,
) -> Result<Vec<(f64, f64)>> {
    let duration = 4.0_f64.max(usize_as_f64(N)?);
    let dt = duration / f64::from(TRANSIENT_POINTS - 1);
    let mut filter = build_filter::<N>(topology, response, InputModel::CurrentHold, 0.0)?;
    let mut points = Vec::with_capacity(TRANSIENT_POINTS as usize);
    points.push((0.0, filter.output()));
    let mut time = 0.0;
    for _ in 1..TRANSIENT_POINTS {
        let output = filter.update(1.0, dt)?;
        time += dt;
        points.push((time, output));
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use ssfilt::Response;

    use super::measure_step_response;
    use crate::Topology;

    #[test]
    fn each_topology_has_a_public_streamed_step_trace() {
        for topology in [Topology::LowPass, Topology::HighPass, Topology::BandPass] {
            let points = measure_step_response::<2>(topology, Response::Butterworth).unwrap();
            assert_eq!(points.len(), 1_601);
            assert_eq!(points.first().unwrap().0, 0.0);
            assert!(
                points
                    .iter()
                    .all(|(time, value)| time.is_finite() && value.is_finite())
            );
        }
    }

    #[test]
    fn high_pass_step_uses_the_new_endpoint_input() {
        let points =
            measure_step_response::<1>(Topology::HighPass, Response::RepeatedPole).unwrap();
        assert!(points[1].1 > 0.95);
        assert!(points[1].1 <= 1.0);
        assert!(points.last().unwrap().1.abs() < 1.0e-4);
    }
}
