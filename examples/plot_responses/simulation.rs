use std::error::Error;

use num_traits::ToPrimitive;
use ssfilt::{InputModel, LowPass, Response};

const CUTOFF_HZ: f64 = 1.0;
const TRANSIENT_POINTS: u32 = 1_601;

pub(super) fn measure_step_response<const N: usize>(
    response: Response,
) -> Result<Vec<(f64, f64)>, Box<dyn Error + Send + Sync>> {
    let order = N.to_f64().ok_or("order does not fit in f64")?;
    let duration = 4.0_f64.max(order);
    let dt = duration / (CUTOFF_HZ * f64::from(TRANSIENT_POINTS - 1));
    let mut filter = LowPass::<N>::builder(CUTOFF_HZ)
        .response(response)
        .input_model(InputModel::CurrentHold)
        .build()?;
    let mut points = Vec::with_capacity(TRANSIENT_POINTS as usize);
    points.push((0.0, filter.output()));
    for index in 1..TRANSIENT_POINTS {
        let output = filter.update(1.0, dt)?;
        points.push((f64::from(index) * dt * CUTOFF_HZ, output));
    }
    Ok(points)
}
