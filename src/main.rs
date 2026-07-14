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
extern crate rustc_hir_analysis;
extern crate rustc_hir_id;
extern crate rustc_interface;
extern crate rustc_lexer;
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
mod callgraph_cache;
mod capability;
mod capsmap;
mod cargo_targets;
mod fs_guard;
mod function_classification;
mod infer_commands;
mod inference;
mod lints;
mod offline_caps;
mod rename;
mod report_commands;
mod setup;
mod symbols;
#[cfg(test)]
mod test_support;
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
            lint_store.register_late_pass(|_| Box::new(lints::RivusLintPass::rvs_new_BIS()));
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

        let wrapper_mode = args.get(1).is_some_and(|arg| rvs_is_rustc_arg(arg));
        if wrapper_mode {
            args.remove(1);
        }

        // In wrapper mode, replace --cap-lints allow with --cap-lints warn
        // so our lint pass runs on std/core/alloc (cargo passes --cap-lints
        // allow for them, which causes rustc to skip the lint pass entirely).
        // Using --cap-lints warn (not removing entirely) prevents compilation
        // failures from std's #[deny(...)] attributes.
        let rivus_enabled = wrapper_mode && rvs_rivus_enabled_BS();
        if rivus_enabled {
            rvs_rewrite_cap_lints_allow_M(&mut args);
        }
        let callgraph_lint_mode = rvs_callgraph_lint_mode(
            wrapper_mode,
            rivus_enabled,
            rvs_env_flag_is_one_BS("RIVUS_CALLGRAPH"),
        );
        rvs_add_callgraph_lint_args_M(&mut args, callgraph_lint_mode);

        if rivus_enabled {
            rustc_driver::run_compiler(&args, &mut RivusCallbacks)
        } else {
            rustc_driver::run_compiler(&args, &mut DefaultCallbacks)
        }
    })
}

fn rvs_rivus_enabled_BS() -> bool {
    match env::var("RIVUS_ENABLED") {
        Ok(value) => rvs_rivus_enabled_value(&value),
        Err(env::VarError::NotPresent) | Err(env::VarError::NotUnicode(_)) => false,
    }
}

fn rvs_env_flag_is_one_BS(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value == "1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallgraphLintMode {
    Normal,
    Collect,
}

fn rvs_callgraph_lint_mode(
    wrapper_mode: bool,
    rivus_enabled: bool,
    collection_requested: bool,
) -> CallgraphLintMode {
    if wrapper_mode && rivus_enabled && collection_requested {
        CallgraphLintMode::Collect
    } else {
        CallgraphLintMode::Normal
    }
}

fn rvs_add_callgraph_lint_args_M(args: &mut Vec<String>, mode: CallgraphLintMode) {
    if mode == CallgraphLintMode::Collect {
        rvs_force_cap_lints_warn_M(args);
        args.push("-Awarnings".to_string());
        args.push("-Aunfulfilled_lint_expectations".to_string());
    }
}

fn rvs_force_cap_lints_warn_M(args: &mut Vec<String>) {
    let mut found = false;
    let mut index = 0usize;
    while index < args.len() {
        if args.get(index).is_some_and(|arg| arg == "--cap-lints") {
            if let Some(value) = args.get_mut(index + 1) {
                *value = "warn".to_string();
                found = true;
            }
            index += 2;
            continue;
        }
        if args
            .get(index)
            .is_some_and(|arg| arg.starts_with("--cap-lints="))
            && let Some(arg) = args.get_mut(index)
        {
            *arg = "--cap-lints=warn".to_string();
            found = true;
        }
        index += 1;
    }
    if !found {
        args.push("--cap-lints=warn".to_string());
    }
}

fn rvs_rivus_enabled_value(value: &str) -> bool {
    value == "1"
}

fn rvs_is_rustc_arg(arg: &str) -> bool {
    std::path::Path::new(arg)
        .file_name()
        .is_some_and(|name| name == "rustc" || name == "rustc.exe")
}

fn rvs_should_enter_driver(args: &[String]) -> bool {
    args.get(1)
        .is_some_and(|arg| arg == "--rustc" || rvs_is_rustc_arg(arg))
}

fn rvs_rewrite_cap_lints_allow_M(args: &mut [String]) {
    for arg in args.iter_mut() {
        if arg == "--cap-lints=allow" {
            *arg = "--cap-lints=warn".to_string();
        }
    }
    for index in 0..args.len().saturating_sub(1) {
        let is_cap_lints_allow = args.get(index..index + 2).is_some_and(
            |pair| matches!(pair, [flag, value] if flag == "--cap-lints" && value == "allow"),
        );
        if is_cap_lints_allow && let Some(value) = args.get_mut(index + 1) {
            *value = "warn".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260706_rewrite_cap_lints_preserves_allow_crate_name() {
        let mut args = vec![
            "rustc".to_string(),
            "--crate-name".to_string(),
            "allow".to_string(),
            "--cap-lints".to_string(),
            "allow".to_string(),
        ];

        rvs_rewrite_cap_lints_allow_M(&mut args);
        let output = format!("{}\n", args.join("\n"));
        rvs_snapshot_BIS(
            "test_20260706_rewrite_cap_lints_preserves_allow_crate_name",
            &output,
        );

        assert_eq!(args[2], "allow");
        assert_eq!(args[4], "warn");
    }

    #[test]
    fn test_20260706_rewrite_cap_lints_equals_allow() {
        let mut args = vec!["rustc".to_string(), "--cap-lints=allow".to_string()];

        rvs_rewrite_cap_lints_allow_M(&mut args);
        let output = format!("{}\n", args.join("\n"));
        rvs_snapshot_BIS("test_20260706_rewrite_cap_lints_equals_allow", &output);

        assert_eq!(args[1], "--cap-lints=warn");
    }

    #[test]
    fn test_20260706_rivus_enabled_requires_one() {
        let rows = [("", false), ("0", false), ("1", true), ("true", false)];
        let output = rows
            .iter()
            .map(|(value, _)| format!("{value:?}={}", rvs_rivus_enabled_value(value)))
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS("test_20260706_rivus_enabled_requires_one", &output);

        for (value, expected) in rows {
            assert_eq!(rvs_rivus_enabled_value(value), expected);
        }
    }

    #[test]
    fn test_20260706_driver_gate_detects_wrapper_args() {
        let cli = vec!["cargo-rivus".to_string(), "usage".to_string()];
        let direct = vec!["cargo-rivus".to_string(), "--rustc".to_string()];
        let wrapper = vec![
            "cargo-rivus".to_string(),
            "/toolchain/bin/rustc".to_string(),
        ];
        let windows_wrapper = vec!["cargo-rivus".to_string(), "rustc.exe".to_string()];
        let false_positive = vec!["cargo-rivus".to_string(), "rustc.json".to_string()];
        let output = format!(
            "cli={}\ndirect={}\nwrapper={}\nwindows_wrapper={}\nfalse_positive={}\n",
            rvs_should_enter_driver(&cli),
            rvs_should_enter_driver(&direct),
            rvs_should_enter_driver(&wrapper),
            rvs_should_enter_driver(&windows_wrapper),
            rvs_should_enter_driver(&false_positive),
        );
        rvs_snapshot_BIS("test_20260706_driver_gate_detects_wrapper_args", &output);

        assert!(!rvs_should_enter_driver(&cli));
        assert!(rvs_should_enter_driver(&direct));
        assert!(rvs_should_enter_driver(&wrapper));
        assert!(rvs_should_enter_driver(&windows_wrapper));
        assert!(!rvs_should_enter_driver(&false_positive));
        assert!(rvs_is_rustc_arg("/toolchain/bin/rustc"));
        assert!(rvs_is_rustc_arg("rustc.exe"));
        assert!(!rvs_is_rustc_arg("rustc.json"));
    }

    #[test]
    fn test_20260706_disabled_wrapper_does_not_rewrite_cap_lints() {
        let mut args = vec!["rustc".to_string(), "--cap-lints=allow".to_string()];
        let enabled = false;
        if enabled {
            rvs_rewrite_cap_lints_allow_M(&mut args);
        }
        let output = args.join("\n") + "\n";
        rvs_snapshot_BIS(
            "test_20260706_disabled_wrapper_does_not_rewrite_cap_lints",
            &output,
        );

        assert_eq!(args[1], "--cap-lints=allow");
    }

    #[test]
    fn test_20260714_callgraph_suppresses_intermediate_expectations() {
        let mut enabled = vec!["rustc".to_string()];
        let mut existing_cap = vec![
            "rustc".to_string(),
            "--cap-lints".to_string(),
            "allow".to_string(),
        ];
        let mut disabled = enabled.clone();
        rvs_add_callgraph_lint_args_M(&mut enabled, CallgraphLintMode::Collect);
        rvs_add_callgraph_lint_args_M(&mut existing_cap, CallgraphLintMode::Collect);
        rvs_add_callgraph_lint_args_M(&mut disabled, CallgraphLintMode::Normal);
        let output = format!(
            "enabled={}\nexisting_cap={}\ndisabled={}\n",
            enabled.join(" "),
            existing_cap.join(" "),
            disabled.join(" ")
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_suppresses_intermediate_expectations",
            &output,
        );

        assert_eq!(
            enabled
                .get(enabled.len().saturating_sub(2))
                .map(String::as_str),
            Some("-Awarnings")
        );
        assert_eq!(
            enabled.last().map(String::as_str),
            Some("-Aunfulfilled_lint_expectations")
        );
        assert_eq!(disabled, ["rustc"]);
        assert!(enabled.iter().any(|arg| arg == "--cap-lints=warn"));
        assert_eq!(existing_cap.get(2).map(String::as_str), Some("warn"));
        assert_eq!(
            rvs_callgraph_lint_mode(true, false, true),
            CallgraphLintMode::Normal
        );
        assert_eq!(
            rvs_callgraph_lint_mode(true, true, true),
            CallgraphLintMode::Collect
        );
    }
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
        /// Output path for direct external deps capsmap under caps/
        #[arg(short = 'o', long = "output", required = true)]
        output: PathBuf,
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
        /// Output path for std capsmap (e.g. caps/std)
        #[arg(short = 'o', long = "output", required = true)]
        output: PathBuf,
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
    let raw_args: Vec<String> = env::args().collect();
    if rvs_should_enter_driver(&raw_args) {
        return rvs_run_driver_BIMPS();
    }

    // Cargo subcommands: `cargo rivus check` invokes `cargo-rivus rivus check`.
    // Strip the leading "rivus" arg so clap sees the real subcommand.
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
            if let Err(code) = workspace::rvs_run_cargo_check_BIMS(&empty_args) {
                process::exit(code);
            }
        }
        Some(Commands::Check { args }) => {
            if let Err(code) = workspace::rvs_run_cargo_check_BIMS(&args) {
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
        Some(Commands::InferCapsmap { path, output }) => {
            if let Err(e) = infer_commands::rvs_run_infer_capsmap_BIMPS(&path, &output) {
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
