//! Searches and reports normalized low-pass phase-equalizer designs.
//!
//! Usage: `cargo run --release --example phase_design -- [--output-dir PATH]
//! [--order 4|8] [--band core|cutoff]`

#[path = "phase_design/render.rs"]
mod render;
#[path = "phase_design/report.rs"]
mod report;
#[path = "phase_design/search.rs"]
mod search;
#[path = "support/svg_smoothing.rs"]
mod svg_smoothing;
#[path = "support/transfer.rs"]
mod transfer;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use search::{Band, Case, CaseResult};

const DEFAULT_OUTPUT_DIR: &str = "target/phase-design";

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    output_dir: PathBuf,
    order: Option<usize>,
    band: Option<Band>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DesignOrder {
    #[value(name = "4")]
    Four,
    #[value(name = "8")]
    Eight,
}

#[derive(Debug, Parser)]
#[command(
    name = "phase_design",
    about = "Search and report normalized low-pass phase-equalizer designs.",
    after_help = "response: Butterworth low-pass\ndefaults: order 4 and 8; bands core and cutoff; output target/phase-design\nbands: core = 0..0.7 * cutoff; cutoff = 0..1.0 * cutoff\nevidence: analytic feasibility only; no streaming equalizer is implemented\nwrites summary.csv, sweep.csv, summary.md, and one SVG dashboard per case"
)]
struct Cli {
    /// Directory for CSV, Markdown, and SVG reports.
    #[arg(long, value_name = "PATH", default_value = DEFAULT_OUTPUT_DIR)]
    output_dir: PathBuf,
    /// Restrict the design study to one supported filter order.
    #[arg(long, value_enum, value_name = "ORDER")]
    order: Option<DesignOrder>,
    /// Restrict the design study to the core or cutoff-wide band.
    #[arg(long, value_enum, value_name = "BAND")]
    band: Option<Band>,
}

impl Cli {
    fn normalize(self) -> Arguments {
        Arguments {
            output_dir: self.output_dir,
            order: self.order.map(|order| match order {
                DesignOrder::Four => 4,
                DesignOrder::Eight => 8,
            }),
            band: self.band,
        }
    }
}

fn main() -> anyhow::Result<()> {
    let arguments = Cli::parse().normalize();

    let mut results = Vec::new();
    for case in selected_cases(arguments.order, arguments.band) {
        let result = search::design(case)
            .map_err(anyhow::Error::from_boxed)
            .with_context(|| {
                format!(
                    "designing {} order {} {} band",
                    case.response_label,
                    case.order,
                    case.band.label()
                )
            })?;
        print_case_result(&result);
        results.push(result);
    }

    report::write_reports(&arguments.output_dir, &results).with_context(|| {
        format!(
            "writing phase-design reports to {}",
            arguments.output_dir.display()
        )
    })?;
    let dashboards = render::write_dashboards(&arguments.output_dir, &results)
        .map_err(anyhow::Error::from_boxed)
        .with_context(|| {
            format!(
                "writing phase-design dashboards to {}",
                arguments.output_dir.display()
            )
        })?;
    println!(
        "wrote {}",
        arguments.output_dir.join("summary.csv").display()
    );
    println!("wrote {}", arguments.output_dir.join("sweep.csv").display());
    println!(
        "wrote {}",
        arguments.output_dir.join("summary.md").display()
    );
    for dashboard in dashboards {
        println!("wrote {}", dashboard.display());
    }
    Ok(())
}

fn selected_cases(order: Option<usize>, band: Option<Band>) -> Vec<Case> {
    let mut cases = Vec::with_capacity(4);
    for candidate_order in [4, 8] {
        if order.is_some_and(|selected| selected != candidate_order) {
            continue;
        }
        for candidate_band in [Band::Core, Band::Cutoff] {
            if band.is_some_and(|selected| selected != candidate_band) {
                continue;
            }
            cases.push(Case {
                response: ssfilt::Response::Butterworth,
                response_label: "Butterworth",
                order: candidate_order,
                band: candidate_band,
            });
        }
    }
    cases
}

fn print_case_result(result: &CaseResult) {
    let case = result.case;
    if result.gates.passes() {
        println!(
            "{} order {} {}: ANALYTIC GATES PASS, NOT STREAMING (peak delay residual {})",
            case.response_label,
            case.order,
            case.band.label(),
            report::format_float(result.corrected_metrics.peak_residual),
        );
    } else {
        let failed = report::failed_gate_names(result.gates).join(", ");
        println!(
            "{} order {} {}: ANALYTIC GATES REJECT, NOT STREAMING (failed: {failed})",
            case.response_label,
            case.order,
            case.band.label(),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{Parser, error::ErrorKind};

    use super::{Arguments, Cli, DEFAULT_OUTPUT_DIR, selected_cases};
    use crate::search::Band;

    fn args(values: &[&str]) -> Result<Arguments, String> {
        let mut argv = vec!["phase_design"];
        argv.extend_from_slice(values);
        let cli = Cli::try_parse_from(argv).map_err(|error| error.to_string())?;
        Ok(cli.normalize())
    }

    #[test]
    fn defaults_select_the_full_matrix_and_default_output_directory() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.output_dir, PathBuf::from(DEFAULT_OUTPUT_DIR));
        assert_eq!(parsed.order, None);
        assert_eq!(parsed.band, None);

        let cases = selected_cases(parsed.order, parsed.band);
        assert_eq!(cases.len(), 4);
        assert_eq!(
            cases
                .iter()
                .map(|case| (case.order, case.band))
                .collect::<Vec<_>>(),
            [
                (4, Band::Core),
                (4, Band::Cutoff),
                (8, Band::Core),
                (8, Band::Cutoff),
            ]
        );
    }

    #[test]
    fn named_options_narrow_the_selected_cases() {
        let parsed = args(&[
            "--output-dir",
            "out dir",
            "--order",
            "8",
            "--band",
            "cutoff",
        ])
        .unwrap();
        assert_eq!(parsed.output_dir, PathBuf::from("out dir"));
        assert_eq!(parsed.order, Some(8));
        assert_eq!(parsed.band, Some(Band::Cutoff));
        let cases = selected_cases(parsed.order, parsed.band);
        assert_eq!(cases.len(), 1);
        assert_eq!((cases[0].order, cases[0].band), (8, Band::Cutoff));
    }

    #[test]
    fn generated_help_lists_choices_defaults_and_evidence_scope() {
        for flag in ["-h", "--help"] {
            let error = Cli::try_parse_from(["phase_design", flag]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            let help = error.to_string();
            assert!(help.contains("--output-dir"));
            assert!(help.contains("--order"));
            assert!(help.contains("4, 8"));
            assert!(help.contains("--band"));
            assert!(help.contains("core, cutoff"));
            assert!(help.contains("target/phase-design"));
            assert!(help.contains("analytic feasibility only"));
            assert!(help.contains("one SVG dashboard per case"));
        }
    }

    #[test]
    fn parser_rejects_duplicate_unknown_invalid_and_positional_arguments() {
        for invalid in [
            vec!["--output-dir", "one", "--output-dir", "two"],
            vec!["--order", "4", "--order", "8"],
            vec!["--band", "core", "--band", "cutoff"],
            vec!["--order", "5"],
            vec!["--band", "wide"],
            vec!["--unknown"],
            vec!["extra"],
            vec!["first", "second"],
            vec!["--output-dir"],
            vec!["--order"],
            vec!["--band"],
            vec!["--output-dir", ""],
        ] {
            assert!(args(&invalid).is_err(), "accepted {invalid:?}");
        }
    }
}
