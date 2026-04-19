use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use rivus_linter::capsmap::CapsMap;
use rivus_linter::rvs_check_mir_dir_BEIM;
use rivus_linter::rvs_check_mir_path_BEIMP;
use rivus_linter::rvs_check_path_BEI;
use rivus_linter::rvs_report_path_BEI;

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

#[allow(non_snake_case)]
fn rvs_main_BEIMP() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { path, capsmap } => {
            let cm = rvs_load_capsmap_BEIP(capsmap);

            match rvs_check_path_BEI(&path, &cm) {
                Ok(output) => {
                    rvs_print_check_output_BIP(&output);
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
            let cm = rvs_load_capsmap_BEIP(capsmap);

            let result = if let Some(dir) = mir_dir {
                rvs_check_mir_dir_BEIM(&dir, &cm)
            } else {
                rvs_check_mir_path_BEIMP(&path, &cm)
            };

            match result {
                Ok(output) => {
                    rvs_print_check_output_BIP(&output);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(2);
                }
            }
        }
        Command::Report { path } => match rvs_report_path_BEI(&path) {
            Ok(report) => print!("{report}"),
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(2);
            }
        },
    }
}

#[allow(non_snake_case)]
fn rvs_load_capsmap_BEIP(capsmap: Option<PathBuf>) -> CapsMap {
    match capsmap {
        Some(cm_path) => {
            let content = std::fs::read_to_string(&cm_path).unwrap_or_else(|e| {
                eprintln!("Error: cannot read capsmap '{}': {e}", cm_path.display());
                process::exit(2);
            });
            CapsMap::rvs_parse_E(&content).unwrap_or_else(|e| {
                eprintln!("Error: invalid capsmap '{}': {e}", cm_path.display());
                process::exit(2);
            })
        }
        None => CapsMap::rvs_new(),
    }
}

#[allow(non_snake_case)]
fn rvs_print_check_output_BIP(output: &rivus_linter::CheckOutput) {
    for w in &output.warnings {
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
    rvs_main_BEIMP();
}
