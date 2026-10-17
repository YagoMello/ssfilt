use std::error::Error;

use ssfilt::{BuildError, LowPass, Response, UpdateError};

#[derive(Clone, Copy)]
enum FilterOrder {
    Two,
    Four,
}

enum RuntimeLowPass {
    Order2(LowPass<2, f32>),
    Order4(LowPass<4, f32>),
}

impl RuntimeLowPass {
    fn build(order: FilterOrder, cutoff_hz: f32, initial_input: f32) -> Result<Self, BuildError> {
        match order {
            FilterOrder::Two => LowPass::<2, f32>::builder(cutoff_hz)
                .response(Response::Butterworth)
                .initial_input(initial_input)
                .build()
                .map(Self::Order2),
            FilterOrder::Four => LowPass::<4, f32>::builder(cutoff_hz)
                .response(Response::Butterworth)
                .initial_input(initial_input)
                .build()
                .map(Self::Order4),
        }
    }

    fn update(&mut self, input: f32, dt_seconds: f32) -> Result<f32, UpdateError> {
        match self {
            Self::Order2(filter) => filter.update(input, dt_seconds),
            Self::Order4(filter) => filter.update(input, dt_seconds),
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let configured_order = match std::env::args().nth(1).as_deref() {
        Some("2") => FilterOrder::Two,
        Some("4") | None => FilterOrder::Four,
        Some(_) => return Err("order must be 2 or 4".into()),
    };

    // The enum stores either concrete filter inline: selecting an order does
    // not require Box, dyn dispatch, or allocation in the filtering path.
    let mut filter = RuntimeLowPass::build(configured_order, 20.0, 0.0)?;
    let output = filter.update(1.0, 0.012)?;
    println!("filtered={output:.6}");
    Ok(())
}
