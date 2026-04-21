use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use rivus_linter::capsmap::CapsMap;
use rivus_linter::rvs_check_mir_dir_BIM;
use rivus_linter::rvs_check_mir_path_BIMPS;
use rivus_linter::rvs_check_path_BI;
use rivus_linter::rvs_report_path_BI;

#[derive(Parser)]
#[command(name = "rivus-linter")]
#[command(about = "Check function capability compliance in Rust source code")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check capability compliance using syn-based source analysis
    Check {
        /// Path to file or directory to check
        path: PathBuf,
        /// Path to capsmap file mapping non-rvs functions to capabilities
        #[arg(short = 'm', long = "capsmap")]
        capsmap: Option<PathBuf>,
    },
    /// Check capability compliance using MIR-based analysis (compile + check)
    MirCheck {
        /// Path to project directory containing Cargo.toml
        path: PathBuf,
        /// Path to capsmap file mapping non-rvs functions to capabilities
        #[arg(short = 'm', long = "capsmap")]
        capsmap: Option<PathBuf>,
        /// Path to directory containing pre-compiled .mir files (skips cargo build)
        #[arg(long = "mir-dir")]
        mir_dir: Option<PathBuf>,
    },
    /// Report line-count breakdown by capability
    Report {
        /// Path to file or directory to analyze
        path: PathBuf,
    },
}

/// # Panics
///
/// Panics on invalid CLI arguments or failed subcommand execution.
#[allow(non_snake_case)]
fn rvs_main_BIMPS() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { path, capsmap } => {
            let cm = rvs_load_capsmap_BIPS(capsmap);

            match rvs_check_path_BI(&path, &cm) {
                Ok(output) => {
                    rvs_print_check_output_BIPS(&output);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(2);
                }
            }
        }
        Command::MirCheck {
            path,
            capsmap,
            mir_dir,
        } => {
            let cm = rvs_load_capsmap_BIPS(capsmap);

            let result = if let Some(dir) = mir_dir {
                rvs_check_mir_dir_BIM(&dir, &cm)
            } else {
                rvs_check_mir_path_BIMPS(&path, &cm)
            };

            match result {
                Ok(output) => {
                    rvs_print_check_output_BIPS(&output);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(2);
                }
            }
        }
        Command::Report { path } => match rvs_report_path_BI(&path) {
            Ok(report) => print!("{report}"),
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(2);
            }
        },
    }
}

/// # Panics
///
/// Panics if the capsmap file is unreadable or contains invalid entries.
#[allow(non_snake_case)]
fn rvs_load_capsmap_BIPS(capsmap: Option<PathBuf>) -> CapsMap {
    match capsmap {
        Some(cm_path) => {
            let content = std::fs::read_to_string(&cm_path).unwrap_or_else(|e| {
                eprintln!("Error: cannot read capsmap '{}': {e}", cm_path.display());
                process::exit(2);
            });
            CapsMap::rvs_parse(&content).unwrap_or_else(|e| {
                eprintln!("Error: invalid capsmap '{}': {e}", cm_path.display());
                process::exit(2);
            })
        }
        None => CapsMap::rvs_new(),
    }
}

/// # Panics
///
/// Panics on I/O errors when writing to stderr.
#[allow(non_snake_case)]
fn rvs_print_check_output_BIPS(output: &rivus_linter::CheckOutput) {
    for w in &output.warnings {
        eprintln!("{w}");
    }
    for w in &output.assert_warnings {
        eprintln!("{w}");
    }
    for w in &output.dead_code_warnings {
        eprintln!("{w}");
    }
    for w in &output.inference_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_allow_warnings {
        eprintln!("{w}");
    }
    for w in &output.test_name_warnings {
        eprintln!("{w}");
    }
    for w in &output.duplicate_test_warnings {
        eprintln!("{w}");
    }
    for w in &output.banned_import_warnings {
        eprintln!("{w}");
    }
    for w in &output.non_rvs_fn_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_doc_warnings {
        eprintln!("{w}");
    }
    for w in &output.deny_warnings_warnings {
        eprintln!("{w}");
    }
    for w in &output.wildcard_import_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_safety_doc_warnings {
        eprintln!("{w}");
    }
    for w in &output.borrowed_param_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_debug_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_panics_doc_warnings {
        eprintln!("{w}");
    }
    for w in &output.into_impl_warnings {
        eprintln!("{w}");
    }
    for w in &output.consumed_arg_on_error_warnings {
        eprintln!("{w}");
    }
    for w in &output.deref_polymorphism_warnings {
        eprintln!("{w}");
    }
    for w in &output.reflection_usage_warnings {
        eprintln!("{w}");
    }
    for w in &output.stub_warnings {
        eprintln!("{w}");
    }
    for w in &output.empty_fn_warnings {
        eprintln!("{w}");
    }
    for w in &output.todo_comment_warnings {
        eprintln!("{w}");
    }
    for w in &output.untested_good_fn_warnings {
        eprintln!("{w}");
    }
    for v in &output.violations {
        eprintln!("{v}");
    }
    if !output.violations.is_empty() {
        process::exit(1);
    }
}

#[allow(non_snake_case)]
fn main() {
    rvs_main_BIMPS();
}
