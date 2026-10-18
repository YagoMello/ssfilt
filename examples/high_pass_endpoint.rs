//! Show how a high-pass output is evaluated at the new sample's endpoint.
//!
//! Run with `cargo run --example high_pass_endpoint`.

use std::error::Error;

use ssfilt::{HighPass, InputModel, Response};

fn main() -> Result<(), Box<dyn Error>> {
    let mut filter = HighPass::<1>::builder(1.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .initial_input(0.0)
        .build()?;

    println!("timestamp_ms,input,high_pass_output");

    let first_output = filter.update(1.0, 0.1)?;
    // The preceding zero is held over 0–100 ms, but high-pass direct
    // feedthrough evaluates the returned output using the new input at 100 ms.
    println!("100,1.0,{first_output:.4}");

    let second_output = filter.update(1.0, 0.1)?;
    println!("200,1.0,{second_output:.4}");

    Ok(())
}
