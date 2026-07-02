#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_hir_id;
extern crate rustc_interface;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::env;
use std::path::PathBuf;
use std::process::{self, ExitCode};

use clap::{Parser, Subcommand};
use rustc_driver::Callbacks;
use rustc_interface::interface;
use rustc_session::EarlyDiagCtxt;
use rustc_session::config::ErrorOutputType;
mod analysis_commands;
mod artifacts;
mod capability;
mod capsmap;
mod infer_commands;
mod inference;
mod lints;
mod rename;
mod report_commands;
mod setup;
mod symbols;
mod workspace;

const RIVUS_MD: &str = include_str!("../rivus.md");
const RIVUS_MANUAL: &str = include_str!("rivus-manual.md");

// ─── Driver mode ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct RivusCallbacks;

impl Callbacks for RivusCallbacks {
    fn config(&mut self, config: &mut interface::Config) {
        let previous = config.register_lints.take();
        config.register_lints = Some(Box::new(move |_sess, lint_store| {
            if let Some(previous) = &previous {
                previous(_sess, lint_store);
            }
            lint_store.register_lints(lints::RIVUS_LINTS);
            lint_store.register_late_pass(|_| Box::new(lints::RivusLintPass::new()));
        }));
        config.opts.unstable_opts.mir_opt_level = Some(0);
    }
}

#[derive(Debug)]
struct DefaultCallbacks;

impl Callbacks for DefaultCallbacks {}

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
fn rvs_run_driver_BIMPS() -> ExitCode {
    let early_dcx = EarlyDiagCtxt::new(ErrorOutputType::default());
    rustc_driver::init_rustc_env_logger(&early_dcx);

    rustc_driver::catch_with_exit_code(move || {
        let mut args: Vec<String> = env::args().collect();

        if args.get(1).is_some_and(|arg| arg == "--rustc") {
            args.remove(1);
            if let Some(arg0) = args.first_mut() {
                *arg0 = "rustc".to_string();
            }
            return rustc_driver::run_compiler(&args, &mut DefaultCallbacks);
        }

        let wrapper_mode = args
            .get(1)
            .map(|s| {
                std::path::Path::new(s)
                    .file_stem()
                    .is_some_and(|stem| stem == "rustc")
            })
            .unwrap_or(false);
        if wrapper_mode {
            args.remove(1);
        }

        // In wrapper mode, replace --cap-lints allow with --cap-lints warn
        // so our lint pass runs on std/core/alloc (cargo passes --cap-lints
        // allow for them, which causes rustc to skip the lint pass entirely).
        // Using --cap-lints warn (not removing entirely) prevents compilation
        // failures from std's #[deny(...)] attributes.
        if wrapper_mode {
            let has_cap_lints_allow = args
                .windows(2)
                .any(|window| matches!(window, [cap_lints, allow] if cap_lints == "--cap-lints" && allow == "allow"));
            if has_cap_lints_allow {
                args.retain(|a| a != "--cap-lints" && a != "allow");
                args.push("--cap-lints".to_string());
                args.push("warn".to_string());
            }
        }

        if env::var("RIVUS_ENABLED").is_ok() {
            rustc_driver::run_compiler(&args, &mut RivusCallbacks)
        } else {
            rustc_driver::run_compiler(&args, &mut DefaultCallbacks)
        }
    })
}

// ─── CLI mode ────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(name = "rivus-linter")]
#[command(about = "Check function capability compliance in Rust source code")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check capability compliance via rustc plugin (cargo check)
    Check {
        /// Path to caps directory
        #[arg(short = 'm', long = "capsmap")]
        capsmap: Option<PathBuf>,
        /// Extra cargo check args
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Report line-count breakdown by capability
    Report {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Set up project: copy rivus.md to AGENTS.md and inject clippy lints into Cargo.toml
    Setup {
        /// Path to target project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Collect callgraph and infer capsmap from seed annotations
    InferCapsmap {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Path to seed caps directory
        #[arg(short = 'm', long = "capsmap", default_value = "caps")]
        capsmap: PathBuf,
        /// Output path for inferred capsmap (default: stdout)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },
    /// Strip rvs_ prefix and capability suffix from all functions
    Strip {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Infer capabilities and add rvs_ prefix and capability suffix to all functions
    Annotate {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Infer capsmap for std/core/alloc via -Zbuild-std (requires nightly)
    InferStd {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output path for std capsmap (default: target/rivus-std-capsmap.txt)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },
    /// Show why a function has its caps (prints callees and their caps)
    Why {
        /// Function def_path to explain (e.g. std::fs::read)
        function: String,
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Display the detailed tool manual
    Usage,
}

fn main() -> ExitCode {
    if env::var("RIVUS_ENABLED").is_ok() {
        return rvs_run_driver_BIMPS();
    }

    // Cargo subcommands: `cargo rivus check` invokes `cargo-rivus rivus check`.
    // Strip the leading "rivus" arg so clap sees the real subcommand.
    let raw_args: Vec<String> = env::args().collect();
    let filtered_args: Vec<String> = if raw_args.get(1).map(|s| s.as_str()) == Some("rivus") {
        let mut v = raw_args;
        v.remove(1);
        v
    } else {
        raw_args
    };
    let cli = Cli::parse_from(filtered_args);

    match cli.command {
        None => {
            let empty_args: Vec<String> = Vec::new();
            let empty_capsmap: Option<PathBuf> = None;
            if let Err(code) = workspace::rvs_run_cargo_check_BIMS(&empty_capsmap, &empty_args) {
                process::exit(code);
            }
        }
        Some(Commands::Check { capsmap, args }) => {
            if let Err(code) = workspace::rvs_run_cargo_check_BIMS(&capsmap, &args) {
                process::exit(code);
            }
        }
        Some(Commands::Report { path }) => {
            if let Err(e) = report_commands::rvs_run_report_BIMPS(&path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Setup { path }) => {
            if let Err(e) = setup::rvs_run_setup_BIMS(&path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::InferCapsmap {
            path,
            capsmap,
            output,
        }) => {
            if let Err(e) = infer_commands::rvs_run_infer_capsmap_BIMPS(&path, &capsmap, &output) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::InferStd { path, output }) => {
            if let Err(e) = infer_commands::rvs_run_infer_std_BIMPS(&path, &output) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Strip { path }) => {
            if let Err(e) = rename::rvs_strip_BIS(&path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Annotate { path }) => {
            if let Err(e) = analysis_commands::rvs_run_annotate_BIMPS(&path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Why { function, path }) => {
            if let Err(e) = analysis_commands::rvs_run_why_BIMPS(&function, &path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Usage) => {
            print!("{RIVUS_MANUAL}");
        }
    }
    ExitCode::SUCCESS
}
