#![forbid(unsafe_code)]

use forward_backward_oracle::{ButterworthModes, RepresentedRecord, evaluate};
use rug::Float;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        None | Some("verify") => verify(),
        Some("session-smoke") => session_smoke(),
        Some("-h" | "--help" | "help") => {
            print_help();
            Ok(())
        }
        Some(argument) => Err(format!("unknown command `{argument}`; use --help").into()),
    }
}

fn session_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let summary = forward_backward_oracle::run_session_smoke().map_err(std::io::Error::other)?;
    println!("{summary}");
    Ok(())
}

fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let record = RepresentedRecord::from_f64(
        1.0,
        vec![0.031_25, 0.007_812_5, 0.023_437_5, 0.015_625],
        vec![-0.4, 0.7, 1.2, -0.3, 0.1],
    )?;

    println!("forward-backward modal oracle self-check");
    println!("order,alpha,gain_at_cutoff,max_192_vs_256,max_imaginary_256");
    for order in [1, 2, 4, 8] {
        let modes = ButterworthModes::new(order, 256);
        let gain = modes.kernel_transform(&Float::with_val(256, 1));
        let low = evaluate(order, &record, 192);
        let high = evaluate(order, &record, 256);
        let maximum_difference = low
            .values()
            .iter()
            .zip(high.values())
            .map(|(left, right)| Float::with_val(256, left - right).abs())
            .fold(Float::new(256), |maximum, difference| {
                if difference > maximum {
                    difference
                } else {
                    maximum
                }
            });

        let mut expected_cutoff_gain = Float::with_val(256, 2);
        expected_cutoff_gain.sqrt_mut();
        expected_cutoff_gain.recip_mut();
        let gain_error = Float::with_val(256, gain.real() - &expected_cutoff_gain).abs();

        let scale = record.scale(256);
        let mut convergence_limit = Float::with_val(256, 1.0e-12);
        convergence_limit *= &scale;
        let mut imaginary_limit = Float::with_val(256, 1.0e-30);
        imaginary_limit *= &scale;

        println!(
            "{order},{},{},{},{}",
            modes.alpha().to_f64(),
            gain.real().to_f64(),
            maximum_difference.to_f64(),
            high.maximum_imaginary_residual().to_f64()
        );

        if gain_error > Float::with_val(256, 2.0e-14) {
            return Err(format!(
                "order {order} cutoff-gain error {} exceeds 2e-14",
                gain_error.to_f64()
            )
            .into());
        }
        if maximum_difference > convergence_limit {
            return Err(format!(
                "order {order} 192/256-bit disagreement {} exceeds 1e-12 times record scale",
                maximum_difference.to_f64()
            )
            .into());
        }
        if high.maximum_imaginary_residual() > &imaginary_limit {
            return Err(format!(
                "order {order} imaginary residual {} exceeds 1e-30 times record scale",
                high.maximum_imaginary_residual().to_f64()
            )
            .into());
        }
    }
    println!("all compact oracle gates passed");
    Ok(())
}

fn print_help() {
    println!(
        "forward-backward-oracle\n\n\
         USAGE:\n  forward-backward-oracle [verify | session-smoke]\n\n\
         COMMANDS:\n\
           verify         run compact 192/256-bit modal and convolution checks (default)\n\
           session-smoke  exercise the root protocol client and its clean shutdown"
    );
}
