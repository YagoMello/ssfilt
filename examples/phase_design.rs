//! Searches and reports normalized low-pass phase-equalizer designs.
//!
//! Usage: `cargo run --release --example phase_design -- [--output-dir PATH]
//! [--order 4|8] [--band core|cutoff]`

#[path = "phase_design/report.rs"]
mod report;
#[path = "phase_design/search.rs"]
mod search;
#[path = "support/transfer.rs"]
mod transfer;

use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;

use search::{Band, Case, CaseResult};

const DEFAULT_OUTPUT_DIR: &str = "target/phase-design";

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    output_dir: PathBuf,
    order: Option<usize>,
    band: Option<Band>,
    help: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(env::args().skip(1)).map_err(invalid_input)?;
    if arguments.help {
        print!("{}", usage());
        return Ok(());
    }

    let mut results = Vec::new();
    for case in selected_cases(arguments.order, arguments.band) {
        let result = search::design(case)?;
        print_case_result(&result);
        results.push(result);
    }

    report::write_reports(&arguments.output_dir, &results)?;
    println!(
        "wrote {}",
        arguments.output_dir.join("summary.csv").display()
    );
    println!("wrote {}", arguments.output_dir.join("sweep.csv").display());
    println!(
        "wrote {}",
        arguments.output_dir.join("summary.md").display()
    );
    Ok(())
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments, String> {
    let mut output_dir = None;
    let mut order = None;
    let mut band = None;
    let mut help = false;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--output-dir" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--output-dir requires a path".to_owned())?;
                if value.is_empty() {
                    return Err("--output-dir requires a non-empty path".to_owned());
                }
                if output_dir.replace(PathBuf::from(value)).is_some() {
                    return Err("--output-dir may be supplied only once".to_owned());
                }
            }
            "--order" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--order requires 4 or 8".to_owned())?;
                let parsed = match value.as_str() {
                    "4" => 4,
                    "8" => 8,
                    _ => return Err(format!("invalid order {value:?}; expected 4 or 8")),
                };
                if order.replace(parsed).is_some() {
                    return Err("--order may be supplied only once".to_owned());
                }
            }
            "--band" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--band requires core or cutoff".to_owned())?;
                let parsed = match value.as_str() {
                    "core" => Band::Core,
                    "cutoff" => Band::Cutoff,
                    _ => return Err(format!("invalid band {value:?}; expected core or cutoff")),
                };
                if band.replace(parsed).is_some() {
                    return Err("--band may be supplied only once".to_owned());
                }
            }
            "-h" | "--help" => {
                if help {
                    return Err("help may be requested only once".to_owned());
                }
                help = true;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}"));
            }
            value => return Err(format!("unexpected positional argument: {value}")),
        }
    }

    Ok(Arguments {
        output_dir: output_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_DIR)),
        order,
        band,
        help,
    })
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
            "{} order {} {}: PASS (peak delay residual {})",
            case.response_label,
            case.order,
            case.band.label(),
            report::format_float(result.corrected_metrics.peak_residual),
        );
    } else {
        let failed = report::failed_gate_names(result.gates).join(", ");
        println!(
            "{} order {} {}: no useful design (failed: {failed})",
            case.response_label,
            case.order,
            case.band.label(),
        );
    }
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn usage() -> &'static str {
    "usage: phase_design [--output-dir PATH] [--order 4|8] [--band core|cutoff] [-h|--help]\n\
     response: Butterworth low-pass\n\
     defaults: order 4 and 8; bands core and cutoff; output target/phase-design\n\
     writes summary.csv, sweep.csv, and summary.md\n"
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Arguments, DEFAULT_OUTPUT_DIR, parse_arguments, selected_cases, usage};
    use crate::search::Band;

    fn args(values: &[&str]) -> Result<Arguments, String> {
        parse_arguments(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn defaults_select_the_full_matrix_and_default_output_directory() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.output_dir, PathBuf::from(DEFAULT_OUTPUT_DIR));
        assert_eq!(parsed.order, None);
        assert_eq!(parsed.band, None);
        assert!(!parsed.help);

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
    fn help_aliases_are_recognized() {
        assert!(args(&["-h"]).unwrap().help);
        assert!(args(&["--help"]).unwrap().help);
        assert!(usage().contains("summary.csv, sweep.csv, and summary.md"));
    }

    #[test]
    fn parser_rejects_duplicate_unknown_invalid_and_positional_arguments() {
        for invalid in [
            vec!["--output-dir", "one", "--output-dir", "two"],
            vec!["--order", "4", "--order", "8"],
            vec!["--band", "core", "--band", "cutoff"],
            vec!["--help", "-h"],
            vec!["--order", "2"],
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
