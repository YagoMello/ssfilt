//! Choose an integration policy at runtime without allocating.
//!
//! Run with `cargo run --example runtime_integrator -- [adaptive|rk4]`.

use ssfilt::{BuildError, DormandPrince54, LowPass, Response, RungeKutta4, UpdateError};

#[derive(Clone, Copy)]
enum PolicyChoice {
    Adaptive,
    FixedRk4,
}

enum RuntimeLowPass {
    Adaptive(LowPass<4, f32, DormandPrince54<f32>>),
    FixedRk4(LowPass<4, f32, RungeKutta4<f32>>),
}

impl RuntimeLowPass {
    fn build(choice: PolicyChoice) -> Result<Self, BuildError> {
        match choice {
            PolicyChoice::Adaptive => LowPass::<4, f32>::builder(20.0)
                .response(Response::Butterworth)
                .initial_input(0.0)
                .build()
                .map(Self::Adaptive),
            PolicyChoice::FixedRk4 => LowPass::<4, f32>::builder(20.0)
                .response(Response::Butterworth)
                .initial_input(0.0)
                .integrator(RungeKutta4 {
                    max_step_seconds: 0.000_25,
                    // Covers the longest expected gap, with room for a
                    // smaller model-derived stability step.
                    max_steps: 64,
                })
                .build()
                .map(Self::FixedRk4),
        }
    }

    fn update(&mut self, input: f32, dt_seconds: f32) -> Result<f32, UpdateError> {
        match self {
            Self::Adaptive(filter) => filter.update(input, dt_seconds),
            Self::FixedRk4(filter) => filter.update(input, dt_seconds),
        }
    }
}

fn main() {
    let choice = match std::env::args().nth(1).as_deref() {
        Some("adaptive") | None => PolicyChoice::Adaptive,
        Some("rk4") => PolicyChoice::FixedRk4,
        Some(_) => {
            eprintln!("policy must be adaptive or rk4");
            std::process::exit(2);
        }
    };

    // The enum stores either concrete filter inline. Only this demonstration's
    // argument parsing and printing require std; the wrapper itself allocates
    // nothing and can be used in a no_std application.
    let mut filter = RuntimeLowPass::build(choice).expect("valid filter configuration");
    for (input, dt_seconds) in [(0.4, 0.001), (0.8, 0.001_1), (1.0, 0.010)] {
        let output = filter
            .update(input, dt_seconds)
            .expect("configured work budget covers the sample interval");
        println!("dt={dt_seconds:.4} s, input={input:.1}, output={output:.6}");
    }
}
