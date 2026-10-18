//! Smooth irregular sensor readings using the time between samples.
//!
//! Run with `cargo run --example irregular_sensor`.

use std::error::Error;
use std::time::Duration;

use ssfilt::{InputModel, LowPass, Response};

fn main() -> Result<(), Box<dyn Error>> {
    const CUTOFF_HZ: f64 = 0.7;

    // Milliseconds since startup and a converted environmental temperature.
    let samples: [(u64, f64); 8] = [
        (0, 22.18),
        (183, 22.12),
        (511, 22.25),
        (735, 22.19),
        (1_204, 22.31),
        (1_390, 22.22),
        (1_901, 22.28),
        (2_260, 22.20),
    ];

    let (first_timestamp_ms, first_temperature_c) = samples[0];
    // Start at the first reading's steady state instead of creating a step
    // from an assumed zero input.
    let mut filter = LowPass::<2>::builder(CUTOFF_HZ)
        .response(Response::Butterworth)
        .input_model(InputModel::Linear)
        .initial_input(first_temperature_c)
        .build()?;

    println!("timestamp_ms,input_c,filtered_c");
    println!(
        "{first_timestamp_ms},{first_temperature_c:.2},{:.4}",
        filter.output()
    );

    let mut previous_timestamp_ms = first_timestamp_ms;
    for &(timestamp_ms, temperature_c) in &samples[1..] {
        let elapsed_ms = timestamp_ms
            .checked_sub(previous_timestamp_ms)
            .filter(|elapsed_ms| *elapsed_ms > 0)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "sample timestamps must be strictly increasing",
                )
            })?;
        // as_secs_f64 keeps the Duration's subsecond precision instead of
        // dropping fractional nanoseconds through integer division.
        let dt_seconds = Duration::from_millis(elapsed_ms).as_secs_f64();

        // This update spans the interval ending at timestamp_ms. With Linear,
        // it interpolates from the preceding reading to this endpoint reading.
        let filtered_c = filter.update(temperature_c, dt_seconds)?;
        println!("{timestamp_ms},{temperature_c:.2},{filtered_c:.4}");
        previous_timestamp_ms = timestamp_ms;
    }

    Ok(())
}
