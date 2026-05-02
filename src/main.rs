#![expect(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes (A/B/I/M/P/S/T/U)"
)]
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use rivus_linter::capsmap::CapsMap;
use rivus_linter::report::rvs_report_path_BI;
use rivus_linter::rvs_check_mir_dir_BIM;
use rivus_linter::rvs_check_mir_path_BIMPS;
use rivus_linter::rvs_check_path_BI;
use rivus_linter::setup::rvs_inject_clippy_lints_M;

const RIVUS_MD: &str = include_str!("../rivus.md");

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
    /// Set up project: copy rivus.md to AGENTS.md and inject clippy lints into Cargo.toml
    Setup {
        /// Path to target project directory (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

/// # Panics
///
/// Panics on invalid CLI arguments or failed subcommand execution.
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
        Command::Setup { path } => rvs_setup_BIMPS(&path),
    }
}

/// # Panics
///
/// Panics if the capsmap file is unreadable or contains invalid entries.
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
/// Panics on I/O errors during setup.
fn rvs_setup_BIMPS(path: &std::path::Path) {
    debug_assert!(path.is_dir(), "setup path must be a directory");

    // 1. Copy rivus.md to AGENTS.md
    let agents_md = path.join("AGENTS.md");
    std::fs::write(&agents_md, RIVUS_MD).unwrap_or_else(|e| {
        eprintln!("Error: cannot write '{}': {e}", agents_md.display());
        process::exit(2);
    });
    println!("Written {}", agents_md.display());

    // 2. Inject clippy lints into Cargo.toml
    let cargo_toml_path = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path).unwrap_or_else(|e| {
        eprintln!("Error: cannot read '{}': {e}", cargo_toml_path.display());
        process::exit(2);
    });

    let (new_content, count) = rvs_inject_clippy_lints_M(&content);
    if count > 0 {
        std::fs::write(&cargo_toml_path, &new_content).unwrap_or_else(|e| {
            eprintln!("Error: cannot write '{}': {e}", cargo_toml_path.display());
            process::exit(2);
        });
        println!(
            "Injected {count} clippy lint(s) into {}",
            cargo_toml_path.display()
        );
    } else {
        println!(
            "All clippy lints already present in {}",
            cargo_toml_path.display()
        );
    }
}

/// # Panics
///
/// Panics on I/O errors when writing to stderr.
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
    for w in &output.todo_comment_warnings {
        eprintln!("{w}");
    }
    for w in &output.untested_good_fn_warnings {
        eprintln!("{w}");
    }
    for w in &output.error_swallow_warnings {
        eprintln!("{w}");
    }
    for w in &output.catch_unwind_warnings {
        eprintln!("{w}");
    }
    for w in &output.catch_all_error_variant_warnings {
        eprintln!("{w}");
    }
    for w in &output.missing_test_output_warnings {
        eprintln!("{w}");
    }
    for w in &output.validate_returns_unit_warnings {
        eprintln!("{w}");
    }
    for v in &output.violations {
        eprintln!("{v}");
    }
    if !output.violations.is_empty() {
        process::exit(1);
    }
}

fn main() {
    rvs_main_BIMPS();
}
