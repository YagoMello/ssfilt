#![forbid(unsafe_code)]

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use forward_backward_oracle::{ButterworthModes, CandidateKind, RepresentedRecord, evaluate};
use rug::Float;

#[derive(Debug, Parser)]
#[command(
    name = "forward-backward-oracle",
    about = "Independent development oracle for ssfilt forward-backward filtering"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the compact 192/256-bit modal and convolution checks.
    Verify,
    /// Exercise the root protocol client and its clean shutdown.
    SessionSmoke,
    /// Produce the first filtered eight-cell diagnostic report.
    Matrix {
        /// New directory in which the report artifacts will be published.
        #[arg(long)]
        output: PathBuf,
        /// This slice currently requires `right-adjacent-step`.
        #[arg(long)]
        fixture: String,
        /// This slice currently requires per-pass order 8.
        #[arg(long)]
        order: usize,
        /// This slice currently requires `f64`.
        #[arg(long)]
        scalar: String,
        /// This slice currently requires `pi/32`.
        #[arg(long = "q-max")]
        q_max: String,
        /// This slice currently requires `uneven`.
        #[arg(long)]
        schedule: String,
        /// Private candidate implementation to exercise.
        #[arg(long, value_enum)]
        candidate: CandidateKind,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command.unwrap_or(Command::Verify) {
        Command::Verify => verify(),
        Command::SessionSmoke => session_smoke(),
        Command::Matrix {
            output,
            fixture,
            order,
            scalar,
            q_max,
            schedule,
            candidate,
        } => {
            validate_filtered_selection(&fixture, order, &scalar, &q_max, &schedule)?;
            let summary = forward_backward_oracle::run_filtered_matrix(&output, candidate)
                .map_err(std::io::Error::other)?;
            println!("{summary}");
            Ok(())
        }
    }
}

fn validate_filtered_selection(
    fixture: &str,
    order: usize,
    scalar: &str,
    q_max: &str,
    schedule: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = (fixture, order, scalar, q_max, schedule);
    let expected = ("right-adjacent-step", 8, "f64", "pi/32", "uneven");
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "the complete matrix is not implemented; this diagnostic slice requires --fixture right-adjacent-step --order 8 --scalar f64 --q-max pi/32 --schedule uneven (received fixture={fixture}, order={order}, scalar={scalar}, q-max={q_max}, schedule={schedule})"
    )
    .into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_selection_is_exact_and_does_not_imply_a_complete_matrix() {
        assert!(
            validate_filtered_selection("right-adjacent-step", 8, "f64", "pi/32", "uneven").is_ok()
        );
        assert!(
            validate_filtered_selection("right-adjacent-step", 4, "f64", "pi/32", "uneven")
                .is_err()
        );
    }
}
