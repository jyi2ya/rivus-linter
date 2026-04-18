use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use rivus_linter::capsmap::CapsMap;
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
    /// Check capability compliance of rvs_ function calls
    Check {
        /// Path to file or directory to check
        path: PathBuf,
        /// Path to capsmap file mapping non-rvs functions to capabilities
        #[arg(short = 'm', long = "capsmap")]
        capsmap: Option<PathBuf>,
    },
    /// Report line-count breakdown by capability
    Report {
        /// Path to file or directory to analyze
        path: PathBuf,
    },
}

#[allow(non_snake_case)]
fn rvs_main_BEIP() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { path, capsmap } => {
            let cm = match capsmap {
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
            };

            match rvs_check_path_BEI(&path, &cm) {
                Ok(output) => {
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

fn main() {
    rvs_main_BEIP();
}
