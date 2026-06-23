#![feature(rustc_private)]
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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitCode};

use capsmap::CapsMap;
use clap::{Parser, Subcommand};
use rustc_driver::Callbacks;
use rustc_interface::interface;
use rustc_session::EarlyDiagCtxt;
use rustc_session::config::ErrorOutputType;
use serde::Deserialize;

mod capability;
mod capsmap;
mod lints;
mod rename;
mod setup;

use capability::{Capability, CapabilitySet};
use setup::rvs_inject_clippy_lints_M;

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

        if args.len() > 1 && args[1] == "--rustc" {
            args.remove(1);
            args[0] = "rustc".to_string();
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
                .any(|w| w[0] == "--cap-lints" && w[1] == "allow");
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
        /// Path to capsmap file or directory
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
        /// Path to seed capsmap file or directory
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
            if let Err(code) = rvs_run_cargo_check_BIMPS(None, vec![]) {
                process::exit(code);
            }
        }
        Some(Commands::Check { capsmap, args }) => {
            if let Err(code) = rvs_run_cargo_check_BIMPS(capsmap, args) {
                process::exit(code);
            }
        }
        Some(Commands::Report { path }) => {
            rvs_run_report_BIMPS(&path);
        }
        Some(Commands::Setup { path }) => {
            rvs_run_setup_BIMS(&path);
        }
        Some(Commands::InferCapsmap {
            path,
            capsmap,
            output,
        }) => {
            if let Err(e) = rvs_run_infer_capsmap_BIMPS(&path, &capsmap, output.as_deref()) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::InferStd { path, output }) => {
            if let Err(e) = rvs_run_infer_std_BIMPS(&path, output.as_deref()) {
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
            if let Err(e) = rvs_run_annotate_BIMPS(&path) {
                eprintln!("Error: {e}");
                return ExitCode::from(2u8);
            }
        }
        Some(Commands::Why { function, path }) => {
            if let Err(e) = rvs_run_why_BIMPS(&function, &path) {
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

// ─── Check subcommand ────────────────────────────────────────────────────

/// Resolve the capsmap path for the lint pass.
///
/// Priority: user-provided > project caps/ dir > built-in caps/ dir.
/// Note: target/rivus-inferred-capsmap.txt is NOT used here — it's a
/// partial snapshot from infer-capsmap, not a complete caps source.
fn rvs_resolve_capsmap_BIS(
    cmd: &mut Command,
    user_capsmap: Option<&Path>,
    project_path: &Path,
    self_path: &Path,
) {
    // 1. User-provided capsmap (explicit -m flag)
    if let Some(p) = user_capsmap.filter(|p| p.exists()) {
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .expect("current dir invalid")
                .join(p)
        };
        cmd.env("RIVUS_CAPSMAP", abs);
        return;
    }

    // 2. Project caps/ directory
    let project_caps = project_path.join("caps");
    if project_caps.is_dir() {
        cmd.env("RIVUS_CAPSMAP", project_caps);
        return;
    }

    // 3. Built-in caps/ directory (next to the linter binary)
    let built_in_caps = self_path.parent().and_then(|exe_dir| {
        exe_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|root| root.join("caps"))
    });
    if let Some(dir) = built_in_caps.filter(|p| p.is_dir()) {
        cmd.env("RIVUS_CAPSMAP", dir);
    }
}

/// Configuration for running `cargo check` with the rivus lint pass.
struct CargoCheckConfig<'a> {
    project_path: &'a Path,
    /// Use RUSTC_WRAPPER (wraps all crates) instead of RUSTC_WORKSPACE_WRAPPER (workspace only).
    wrap_all_crates: bool,
    /// Pass --tests to cargo check.
    with_tests: bool,
    /// Use -Zbuild-std with nightly toolchain.
    build_std: bool,
    /// User-provided capsmap path (highest priority).
    user_capsmap: Option<&'a Path>,
    /// Extra environment variables to set.
    extra_env: Vec<(&'a str, String)>,
    /// Extra cargo check arguments.
    extra_args: Vec<&'a str>,
    /// Output subdirectory name under target/ (e.g. "rivus-build", "rivus-report-build").
    /// If None, uses default target/ directory.
    target_subdir: Option<&'a str>,
}

/// Runs `cargo check` with the rivus lint pass configured according to `config`.
/// Returns `Ok(())` on success, `Err(message)` on failure.
///
/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
fn rvs_run_cargo_check_impl_BIMPS(config: &CargoCheckConfig) -> Result<(), String> {
    let self_path = env::current_exe().expect("current executable path invalid");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(&cargo);

    if config.build_std {
        cmd.env("RUSTUP_TOOLCHAIN", "nightly");
    }
    cmd.current_dir(config.project_path);

    let wrapper_env = if config.wrap_all_crates {
        "RUSTC_WRAPPER"
    } else {
        "RUSTC_WORKSPACE_WRAPPER"
    };
    cmd.env(wrapper_env, &self_path).env("RIVUS_ENABLED", "1");

    for (key, val) in &config.extra_env {
        cmd.env(key, val);
    }

    // Resolve capsmap only when not in callgraph-only mode
    // (callgraph-only mode passes capsmap via extra_env as RIVUS_CAPSMAP).
    let has_callgraph_env = config
        .extra_env
        .iter()
        .any(|(k, _)| *k == "RIVUS_CALLGRAPH");
    if !has_callgraph_env {
        rvs_resolve_capsmap_BIS(
            &mut cmd,
            config.user_capsmap,
            config.project_path,
            &self_path,
        );
    }

    cmd.arg("check");
    if config.with_tests {
        cmd.arg("--tests");
    }
    if config.build_std {
        cmd.arg("-Zbuild-std=std,core,alloc");
        cmd.arg("--target").arg(rvs_host_triple_BIMS());
    }
    if let Some(subdir) = config.target_subdir {
        let target_dir = config.project_path.join("target").join(subdir);
        cmd.arg("--target-dir").arg(&target_dir);
    }
    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        return Err(format!(
            "cargo check failed (exit code {:?})",
            exit_status.code()
        ));
    }
    Ok(())
}

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
fn rvs_run_cargo_check_BIMPS(capsmap: Option<PathBuf>, extra_args: Vec<String>) -> Result<(), i32> {
    let project_path = Path::new(".");
    let extra_args_ref: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    match rvs_run_cargo_check_impl_BIMPS(&CargoCheckConfig {
        project_path,
        wrap_all_crates: false,
        with_tests: true,
        build_std: false,
        user_capsmap: capsmap.as_deref(),
        extra_env: vec![],
        extra_args: extra_args_ref,
        target_subdir: None,
    }) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("{e}");
            Err(1)
        }
    }
}

// ─── Report subcommand ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct CapStats {
    fn_count: usize,
    line_count: usize,
}

#[derive(Debug, Clone)]
struct Report {
    by_capability: BTreeMap<Capability, CapStats>,
    pure_fn_count: usize,
    pure_line_count: usize,
    good_fn_count: usize,
    good_line_count: usize,
    ok_fn_count: usize,
    ok_line_count: usize,
    total_fn_count: usize,
    total_line_count: usize,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Capability Report")?;
        writeln!(f, "{:-<60}", "")?;
        writeln!(
            f,
            "Total: {} functions, {} lines",
            self.total_fn_count, self.total_line_count
        )?;
        writeln!(f, "{:-<60}", "")?;

        if self.total_line_count == 0 {
            writeln!(f, "(no rvs_ functions found)")?;
            return Ok(());
        }

        let bar_width = 30;
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        rows.push(("(ok)".to_string(), self.ok_fn_count, self.ok_line_count));
        rows.push((
            "(good)".to_string(),
            self.good_fn_count,
            self.good_line_count,
        ));
        rows.push((
            "(pure)".to_string(),
            self.pure_fn_count,
            self.pure_line_count,
        ));

        for cap in [
            Capability::A,
            Capability::B,
            Capability::I,
            Capability::M,
            Capability::P,
            Capability::S,
            Capability::T,
            Capability::U,
        ] {
            if let Some(stats) = self.by_capability.get(&cap) {
                rows.push((cap.to_string(), stats.fn_count, stats.line_count));
            }
        }
        rows.sort_by_key(|b| std::cmp::Reverse(b.2));

        for (label, fn_count, line_count) in &rows {
            let pct = *line_count as f64 / self.total_line_count as f64 * 100.0;
            #[expect(clippy::cast_sign_loss, reason = "pct is 0..=100")]
            let bar_len = (pct / 100.0 * bar_width as f64)
                .round()
                .clamp(0.0, bar_width as f64) as usize;
            let bar: String = "\u{2588}".repeat(bar_len) + &"\u{2591}".repeat(bar_width - bar_len);
            writeln!(
                f,
                "  {:<12} {:>5} fns {:>6} lines {:>6}% |{}|",
                label,
                fn_count,
                line_count,
                format!("{pct:.1}"),
                bar
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FnEntry {
    capabilities: CapabilitySet,
    line_count: usize,
    is_test: bool,
    allows_dead_code: bool,
}

#[derive(Debug, Deserialize)]
struct JsonReportEntry {
    caps: String,
    lines: usize,
    is_test: bool,
    allows_dead_code: bool,
}

#[derive(Debug, Deserialize)]
struct JsonFnBehavior {
    calls: BTreeSet<String>,
    has_async: bool,
    is_unsafe_fn: bool,
    has_mut_param: bool,
    has_static_ref: bool,
    has_static_mut_ref: bool,
    has_thread_local_ref: bool,
    #[serde(default)]
    is_trait_impl: bool,
    #[serde(default)]
    is_test: bool,
    #[serde(default)]
    is_port_method: bool,
}

fn rvs_build_report(entries: &[FnEntry]) -> Report {
    let mut by_capability: BTreeMap<Capability, CapStats> = BTreeMap::new();
    let mut pure_fn_count = 0usize;
    let mut pure_line_count = 0usize;
    let mut good_fn_count = 0usize;
    let mut good_line_count = 0usize;
    let mut ok_fn_count = 0usize;
    let mut ok_line_count = 0usize;
    let mut total_fn_count = 0usize;
    let mut total_line_count = 0usize;
    let good_allowed = CapabilitySet::rvs_from_good_caps();
    let ok_allowed = CapabilitySet::rvs_from_ok_caps();

    for func in entries {
        if func.is_test || func.allows_dead_code {
            continue;
        }
        total_fn_count += 1;
        total_line_count += func.line_count;

        if func.capabilities.rvs_is_empty() {
            pure_fn_count += 1;
            pure_line_count += func.line_count;
        } else {
            for cap in func.capabilities.rvs_iter() {
                let stats = by_capability.entry(cap).or_default();
                stats.fn_count += 1;
                stats.line_count += func.line_count;
            }
        }

        if func.capabilities.rvs_is_subset_of(&good_allowed) {
            good_fn_count += 1;
            good_line_count += func.line_count;
        }

        if func.capabilities.rvs_is_subset_of(&ok_allowed) {
            ok_fn_count += 1;
            ok_line_count += func.line_count;
        }
    }

    Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        ok_fn_count,
        ok_line_count,
        total_fn_count,
        total_line_count,
    }
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_report_BIMPS(path: &Path) {
    let report_dir = path.join("target").join("rivus-report");
    let abs_report_dir = std::env::current_dir()
        .expect("current dir invalid")
        .join(&report_dir);
    rvs_clean_dir_BIS(&report_dir);
    rvs_clean_dir_BIS(&path.join("target").join("rivus-report-build"));

    if let Err(e) = rvs_run_cargo_check_impl_BIMPS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates: false,
        with_tests: true,
        build_std: false,
        user_capsmap: None,
        extra_env: vec![
            ("RIVUS_REPORT", "1".into()),
            (
                "RIVUS_REPORT_DIR",
                abs_report_dir.to_string_lossy().into_owned(),
            ),
        ],
        extra_args: vec![],
        target_subdir: Some("rivus-report-build"),
    }) {
        // Report mode should still produce output even if lint violations
        // (deny-level errors) cause cargo check to fail. The report JSON
        // is written by the lint pass before compilation aborts.
        eprintln!("warning: {e}");
    }

    let mut all_entries: Vec<FnEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&report_dir) else {
        let report = rvs_build_report(&all_entries);
        print!("{report}");
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|ext| ext == "json") {
            let json_str = std::fs::read_to_string(&p).unwrap_or_else(|e| {
                eprintln!("Error: cannot read {}: {e}", p.display());
                process::exit(2);
            });
            let entries = rvs_parse_report_json_S(&json_str).unwrap_or_else(|e| {
                eprintln!("Error: parsing {}: {e}", p.display());
                process::exit(2);
            });
            all_entries.extend(entries);
        }
    }
    let report = rvs_build_report(&all_entries);
    print!("{report}");
}

fn rvs_parse_report_json_S(json: &str) -> Result<Vec<FnEntry>, String> {
    let raw: Vec<JsonReportEntry> =
        serde_json::from_str(json).map_err(|e| format!("invalid report JSON: {e}"))?;
    Ok(raw
        .into_iter()
        .map(|e| FnEntry {
            capabilities: if e.caps.is_empty() {
                CapabilitySet::rvs_new()
            } else {
                CapabilitySet::rvs_from_validated(&e.caps)
            },
            line_count: e.lines,
            is_test: e.is_test,
            allows_dead_code: e.allows_dead_code,
        })
        .collect())
}

// ─── Setup subcommand ────────────────────────────────────────────────────

fn rvs_run_setup_BIMS(path: &Path) {
    if let Err(e) = rvs_ensure_project_dir_BS(path) {
        eprintln!("Error: {e}");
        process::exit(2);
    }

    let agents_md = path.join("AGENTS.md");
    std::fs::write(&agents_md, RIVUS_MD).unwrap_or_else(|e| {
        eprintln!("Error: cannot write '{}': {e}", agents_md.display());
        process::exit(2);
    });
    println!("Written {}", agents_md.display());

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

// ─── Unified callgraph + caps loading ─────────────────────────────────────

/// Unified callgraph collector.
///
/// Cleans the callgraph output directory and build directory, runs
/// `cargo check` with the callgraph collection environment, and returns
/// the merged callgraph.
///
/// - `build_std=false` → wraps all crates (RUSTC_WRAPPER), uses `target/rivus-build`
/// - `build_std=true`  → wraps all crates + `-Zbuild-std`, uses `target/rivus-build-std`
///
/// `extra_env` is merged into the cargo environment, useful for passing
/// `RIVUS_CAPSMAP` to the lint subprocess.
///
/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_collect_callgraph_BIMPS(
    path: &Path,
    build_std: bool,
    with_tests: bool,
    extra_env: Vec<(&str, String)>,
) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let suffix = if build_std { "-std" } else { "" };
    let cg_subdir = format!("rivus-callgraph{suffix}");
    let build_subdir = format!("rivus-build{suffix}");

    let cg_dir = path.join("target").join(&cg_subdir);
    let abs_cg_dir = std::env::current_dir()
        .expect("current dir invalid")
        .join(&cg_dir);

    rvs_clean_dir_BIS(&cg_dir);
    rvs_clean_dir_BIS(&path.join("target").join(&build_subdir));

    let mut env_vars = vec![
        ("RIVUS_CALLGRAPH", "1".into()),
        (
            "RIVUS_CALLGRAPH_DIR",
            abs_cg_dir.to_string_lossy().into_owned(),
        ),
    ];
    env_vars.extend(extra_env);

    rvs_run_cargo_check_impl_BIMPS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates: true,
        with_tests,
        build_std,
        user_capsmap: None,
        extra_env: env_vars,
        extra_args: vec![],
        target_subdir: Some(&build_subdir),
    })?;

    rvs_merge_callgraph_dir_BIS(&cg_dir)
}

/// Load callgraph from cache, or collect fresh.
///
/// Tries `rivus-callgraph` first, then `rivus-callgraph-std`. If neither
/// exists, collects fresh (non-build-std).
fn rvs_load_or_collect_callgraph_BIMPS(path: &Path) -> BTreeMap<String, ParsedFnBehavior> {
    let cg_dir = path.join("target").join("rivus-callgraph");
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");

    if cg_dir.is_dir() || cg_std_dir.is_dir() {
        let mut merged = BTreeMap::new();
        if cg_dir.is_dir() {
            if let Ok(cg) = rvs_merge_callgraph_dir_BIS(&cg_dir) {
                merged.extend(cg);
            }
        }
        if cg_std_dir.is_dir() {
            if let Ok(cg) = rvs_merge_callgraph_dir_BIS(&cg_std_dir) {
                merged.extend(cg);
            }
        }
        merged
    } else {
        eprintln!("(no cached callgraph found, collecting fresh...)");
        rvs_collect_callgraph_BIMPS(path, false, true, vec![]).unwrap_or_default()
    }
}

/// Load callgraph and caps for a project, used by annotate, why, and similar
/// commands that need inferred capabilities.
///
/// Loads callgraph via `rvs_load_or_collect_callgraph_BIMPS` and caps
/// from `caps/` (excluding `deps`) via `CapsMap::rvs_load_dir_excluding_BIS`.
fn rvs_load_callgraph_and_caps_BIMS(
    path: &Path,
) -> Result<(BTreeMap<String, ParsedFnBehavior>, capsmap::CapsMap), String> {
    let callgraph = rvs_load_or_collect_callgraph_BIMPS(path);
    let caps_dir = path.join("caps");
    let caps = if caps_dir.is_dir() {
        CapsMap::rvs_load_dir_excluding_BIS(&caps_dir, &["deps"]).unwrap_or_else(|e| {
            eprintln!("warning: caps/: {e}");
            CapsMap::rvs_new()
        })
    } else {
        CapsMap::rvs_new()
    };
    Ok((callgraph, caps))
}

fn rvs_write_capsmap_result_BIS(
    result: &str,
    default_path: &Path,
    output: Option<&Path>,
    label: &str,
) -> Result<(), String> {
    std::fs::write(default_path, result)
        .map_err(|e| format!("cannot write {}: {e}", default_path.display()))?;
    match output {
        Some(p) => {
            std::fs::write(p, result).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
            println!("Written {label} to {}", p.display());
        }
        None => print!("{result}"),
    }
    Ok(())
}

fn rvs_detect_crate_name_BIS(path: &Path) -> Result<String, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let doc: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e| format!("invalid TOML in {}: {e}", cargo_toml.display()))?;
    doc["package"]["name"]
        .as_str()
        .map(|s| s.replace('-', "_"))
        .ok_or_else(|| format!("{}: missing [package].name", cargo_toml.display()))
}

fn rvs_clean_dir_BIS(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;

    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    let workspace_name = rvs_detect_crate_name_BIS(path)?;

    let mut renames: Vec<(String, String)> = Vec::new();
    let mut skip_names: HashSet<String> = HashSet::new();
    for (full_path, caps) in &inferred {
        if !full_path.starts_with(&format!("{workspace_name}::")) {
            continue;
        }
        let short_name = full_path.rsplit("::").next().unwrap_or(full_path);
        if short_name.starts_with("rvs_") {
            continue;
        }
        if short_name.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        if full_path == &format!("{workspace_name}::main") {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_test) {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_trait_impl) {
            skip_names.insert(short_name.to_string());
            continue;
        }
        let caps_str = rvs_caps_to_string(caps);
        let new_name = if caps_str.is_empty() {
            format!("rvs_{short_name}")
        } else {
            format!("rvs_{short_name}_{caps_str}")
        };
        renames.push((short_name.to_string(), new_name));
    }

    renames.retain(|(name, _)| !skip_names.contains(name));
    renames.sort();
    renames.dedup();

    if renames.is_empty() {
        println!("No functions to annotate.");
        return Ok(());
    }

    let rename_map: HashMap<String, String> = renames.into_iter().collect();
    let files_changed = rename::rvs_apply_ra_renames_BIS(path, &rename_map)?;

    println!(
        "Annotate complete: renamed {} function(s) in {} file(s).",
        rename_map.len(),
        files_changed
    );
    Ok(())
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_why_BIMPS(function: &str, path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;

    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);
    let impl_index = rvs_build_impl_index(&callgraph);

    // Find the function
    let Some(behavior) = callgraph.get(function) else {
        let candidates: Vec<&String> = callgraph
            .keys()
            .filter(|k| k.contains(function))
            .take(10)
            .collect();
        if candidates.is_empty() {
            return Err(format!("function '{function}' not found in callgraph"));
        }
        eprintln!("Exact match not found. Did you mean:");
        for c in &candidates {
            let caps_str = inferred
                .get(*c)
                .map(|cs| {
                    let s = rvs_caps_to_string(cs);
                    if s.is_empty() {
                        " (pure)".to_string()
                    } else {
                        format!(" = {s}")
                    }
                })
                .unwrap_or_else(|| " (unknown)".to_string());
            eprintln!("  {c}{caps_str}");
        }
        return Ok(());
    };

    // Print the function's own caps
    let own_caps = inferred.get(function);
    let caps_str = match own_caps {
        Some(cs) => {
            let s = rvs_caps_to_string(cs);
            if s.is_empty() {
                " (pure)".to_string()
            } else {
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(" = {s} ({desc})")
            }
        }
        None => " (not in inferred)".to_string(),
    };
    println!("{function}{caps_str}");
    println!();

    if behavior.calls.is_empty() {
        println!("  (no callees)");
        return Ok(());
    }

    // Print each callee and its caps
    let mut callees: Vec<(&String, Option<CapabilitySet>)> = behavior
        .calls
        .iter()
        .map(|callee| {
            let caps = inferred
                .get(callee)
                .cloned()
                .or_else(|| seed.rvs_lookup(callee).cloned())
                .or_else(|| {
                    if !callee.contains('@') {
                        rvs_resolve_impl_union_M(callee, &impl_index, &inferred, &callgraph)
                    } else {
                        None
                    }
                });
            (callee, caps)
        })
        .collect();
    callees.sort_by(|a, b| a.0.cmp(b.0));

    println!("  callees:");
    for (callee, caps) in &callees {
        let s = match caps {
            Some(cs) if !cs.rvs_is_empty() => {
                let chars = rvs_caps_to_string(cs);
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{chars} ({desc})")
            }
            Some(_) => "(pure)".to_string(),
            None => "(unknown)".to_string(),
        };
        println!("    {callee}: {s}");
    }

    Ok(())
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_infer_capsmap_BIMPS(
    path: &Path,
    seed_capsmap: &Path,
    output: Option<&Path>,
) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;

    let abs_seed = if seed_capsmap.is_absolute() {
        seed_capsmap.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("current dir invalid")
            .join(seed_capsmap)
    };

    let callgraph = rvs_collect_callgraph_BIMPS(
        path,
        false,
        false,
        vec![("RIVUS_CAPSMAP", abs_seed.to_string_lossy().into_owned())],
    )?;

    // Load caps for inference, excluding deps (that's what we're regenerating).
    let seed = CapsMap::rvs_load_dir_excluding_BIS(seed_capsmap, &["deps"]).unwrap_or_else(|e| {
        eprintln!("warning: caps: {e}");
        CapsMap::rvs_new()
    });

    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    let all_result = rvs_format_capsmap(&inferred);
    let cache_path = path.join("target").join("rivus-inferred-capsmap.txt");
    std::fs::write(&cache_path, &all_result)
        .map_err(|e| format!("cannot write {}: {e}", cache_path.display()))?;

    let crate_name = rvs_detect_crate_name_BIS(path)?;
    let impl_index = rvs_build_impl_index(&callgraph);
    let (direct_external_calls, unknown_callees) =
        rvs_collect_direct_external_deps(&callgraph, &crate_name, &seed, &inferred, &impl_index);

    if !unknown_callees.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown_callees,
            "error: the following external functions have no capability data.\n\
             Add them to caps/seed or caps/ext with the correct capability markers:\n\n",
        ));
    }

    let deps_result = rvs_format_capsmap(&direct_external_calls);
    let deps_default_path = path.join("target").join("rivus-deps-capsmap.txt");
    match output {
        Some(p) => {
            std::fs::write(p, &deps_result)
                .map_err(|e| format!("cannot write {}: {e}", p.display()))?;
            println!("Written deps capsmap to {}", p.display());
        }
        None => {
            std::fs::write(&deps_default_path, &deps_result)
                .map_err(|e| format!("cannot write {}: {e}", deps_default_path.display()))?;
            print!("{deps_result}");
        }
    }
    Ok(())
}

fn rvs_merge_callgraph_dir_BIS(
    cg_dir: &Path,
) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let mut merged: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
    let cg_entries =
        std::fs::read_dir(cg_dir).map_err(|e| format!("cannot read {}: {e}", cg_dir.display()))?;
    for entry in cg_entries {
        let entry = entry.map_err(|e| format!("readdir error: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let json_str = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let partial = rvs_parse_callgraph_S(&json_str)?;
            for (func, behavior) in partial {
                merged.entry(func).or_default().rvs_merge_M(&behavior);
            }
        }
    }
    Ok(merged)
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_infer_std_BIMPS(path: &Path, output: Option<&Path>) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(format!("'{}' is not a Cargo project", path.display()));
    }

    // Collect callgraph with build-std (unified collector).
    let callgraph = rvs_collect_callgraph_BIMPS(path, true, false, vec![])?;

    // Load seed + suppress only (NOT std/deps/ext — we're regenerating std).
    let caps_dir = path.join("caps");
    let seed =
        CapsMap::rvs_load_dir_layers_BIS(&caps_dir, &["seed", "suppress"]).unwrap_or_else(|e| {
            eprintln!("warning: caps: {e}");
            CapsMap::rvs_new()
        });

    // Pre-generate trait-definition aliases before inference so that
    // propagation sees the correct caps for trait definition paths.
    //
    // In build-std mode, trait method *definitions* appear in the callgraph
    // (they have a body).  But in normal (non-build-std) compilation, core/std
    // are pre-compiled rlibs — the trait definition body is invisible, so the
    // def_path at the call site resolves to `TraitPath::method` which does NOT
    // exist in the callgraph.  The impl path `module::method@TraitPath` *is*
    // present.
    //
    // We generate aliases using majority-vote over std impls only, then inject
    // them into the callgraph as seed entries so propagation uses the correct
    // values.  Only std/core/alloc/compiler_builtins impls participate — we
    // don't want a tokio impl's B/I to pollute a core trait method.
    let std_crates: &[&str] = &["std::", "core::", "alloc::", "compiler_builtins::"];
    let pre_index = rvs_build_impl_index(&callgraph);
    // Quick first-pass inference (signature-only, no propagation) to get
    // impl caps for voting.
    let pre_inferred: BTreeMap<String, CapabilitySet> = {
        let mut m = BTreeMap::new();
        for (func, behavior) in &callgraph {
            if let Some(caps) = seed.rvs_lookup(func) {
                m.insert(func.clone(), caps.clone());
            } else {
                m.insert(func.clone(), rvs_infer_signature_caps(behavior));
            }
        }
        m
    };
    let std_pre_inferred: BTreeMap<String, CapabilitySet> = pre_inferred
        .iter()
        .filter(|(k, _)| std_crates.iter().any(|p| k.starts_with(p)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut alias_seed = seed.clone();
    let pre_aliases = rvs_generate_trait_aliases_MP(&std_pre_inferred, &pre_index, &callgraph);
    for (k, v) in &pre_aliases {
        let caps_str = rvs_caps_to_string(v);
        let line = format!("{k}={caps_str}");
        if let Ok(tmp) = capsmap::CapsMap::rvs_parse(&line) {
            alias_seed.rvs_extend_from_M(tmp);
        }
    }

    let mut inferred = rvs_infer_caps_M(&callgraph, &alias_seed);

    // Also inject aliases into inferred (for the std_only output filter).
    let impl_index = rvs_build_impl_index(&callgraph);
    let std_inferred: BTreeMap<String, CapabilitySet> = inferred
        .iter()
        .filter(|(k, _)| std_crates.iter().any(|p| k.starts_with(p)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let post_aliases = rvs_generate_trait_aliases_MP(&std_inferred, &impl_index, &callgraph);
    inferred.extend(post_aliases);

    // Build std capsmap from callgraph inference.
    let crate_name = rvs_detect_crate_name_BIS(path)?;
    let crate_prefix = format!("{crate_name}::");

    // Collect inferred caps for std/core/alloc/compiler_builtins functions.
    let std_only: BTreeMap<String, CapabilitySet> = inferred
        .iter()
        .filter(|(name, _caps)| {
            !name.starts_with(&crate_prefix)
                && std_crates.iter().any(|prefix| name.starts_with(prefix))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Check for std functions that have callees outside the inferred map.
    // These are functions whose capabilities could not be fully determined.
    let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (func, behavior) in &callgraph {
        let is_std = std_crates.iter().any(|p| func.starts_with(p));
        if !is_std {
            continue;
        }
        for callee in &behavior.calls {
            if inferred.contains_key(callee) {
                continue; // callee has known non-empty caps
            }
            if seed.rvs_lookup(callee).is_some() {
                continue; // callee is in seed
            }
            if callgraph.contains_key(callee) {
                continue; // callee is in callgraph — analyzed, inferred as pure
            }
            // callee is truly unknown — not in callgraph, not in seed, not inferred
            unknown
                .entry(callee.clone())
                .or_default()
                .insert(func.clone());
        }
    }

    if !unknown.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown,
            "error: the following functions are called by std but have no capability data.\n\
             Add them to caps/seed with the correct capability markers:\n\n",
        ));
    }

    let result = rvs_format_capsmap(&std_only);
    let default_path = path.join("target").join("rivus-std-capsmap.txt");
    rvs_write_capsmap_result_BIS(&result, &default_path, output, "std capsmap")
}

/// # Panics
///
/// Panics if `rustc -vV` cannot be executed or returns a non-zero exit status.
fn rvs_host_triple_BIMS() -> String {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("failed to run rustc -vV");
    let status = output.status;
    if !status.success() {
        panic!(
            "rustc -vV failed with exit status {}",
            status.code().unwrap_or(-1)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            return host.trim().to_string();
        }
    }
    "x86_64-unknown-linux-gnu".into()
}

fn rvs_parse_callgraph_S(json: &str) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let raw: BTreeMap<String, JsonFnBehavior> =
        serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.into())).collect())
}

#[derive(Debug, Default)]
struct ParsedFnBehavior {
    calls: BTreeSet<String>,
    has_async: bool,
    is_unsafe_fn: bool,
    has_mut_param: bool,
    has_static_ref: bool,
    has_static_mut_ref: bool,
    has_thread_local_ref: bool,
    is_trait_impl: bool,
    is_test: bool,
    is_port_method: bool,
}

impl ParsedFnBehavior {
    fn rvs_merge_M(&mut self, other: &Self) {
        self.calls.extend(other.calls.iter().cloned());
        self.has_async |= other.has_async;
        self.is_unsafe_fn |= other.is_unsafe_fn;
        self.has_mut_param |= other.has_mut_param;
        self.has_static_ref |= other.has_static_ref;
        self.has_static_mut_ref |= other.has_static_mut_ref;
        self.has_thread_local_ref |= other.has_thread_local_ref;
        self.is_trait_impl |= other.is_trait_impl;
        self.is_test |= other.is_test;
        self.is_port_method |= other.is_port_method;
    }
}

impl From<JsonFnBehavior> for ParsedFnBehavior {
    fn from(j: JsonFnBehavior) -> Self {
        Self {
            calls: j.calls,
            has_async: j.has_async,
            is_unsafe_fn: j.is_unsafe_fn,
            has_mut_param: j.has_mut_param,
            has_static_ref: j.has_static_ref,
            has_static_mut_ref: j.has_static_mut_ref,
            has_thread_local_ref: j.has_thread_local_ref,
            is_trait_impl: j.is_trait_impl,
            is_test: j.is_test,
            is_port_method: j.is_port_method,
        }
    }
}

/// Build a "method@trait_path" → set-of-keys index from callgraph keys.
///
/// Callgraph keys for trait impl methods look like:
///   std::fs::read@std::io::Read
///   kovi_plugin_irc_gateway::config::deserialize@serde::de::Deserialize
///
/// This index allows resolving trait method callees (e.g. `serde::de::Deserializer::deserialize_any`)
/// by finding all impl methods with matching `@trait_path`.
fn rvs_build_impl_index(
    callgraph: &BTreeMap<String, ParsedFnBehavior>,
) -> HashMap<String, Vec<String>> {
    let mut idx: HashMap<String, Vec<String>> = HashMap::new();
    for key in callgraph.keys() {
        if let Some(at_pos) = key.find('@') {
            let suffix = &key[at_pos + 1..];
            let method = &key[..at_pos];
            let method_name = method.rsplit("::").next().unwrap_or(method);
            let lookup = format!("{method_name}@{suffix}");
            idx.entry(lookup).or_default().push(key.clone());
        }
    }
    idx
}

/// Infer capabilities from behavioral flags alone (no propagation).
/// Used by both `rvs_infer_caps_M` and `rvs_run_infer_std_BIMPS`.
fn rvs_infer_signature_caps(behavior: &ParsedFnBehavior) -> CapabilitySet {
    // Port trait methods get ONLY P — no other caps, no signature inference.
    // The whole point of a Port is that callers see a clean interface,
    // not the I/O capabilities of the real implementation behind it.
    if behavior.is_port_method {
        let mut caps = CapabilitySet::rvs_new();
        caps.rvs_insert_M(Capability::P);
        return caps;
    }
    let mut caps = CapabilitySet::rvs_new();
    if behavior.has_async {
        caps.rvs_insert_M(Capability::A);
    }
    if behavior.is_unsafe_fn {
        caps.rvs_insert_M(Capability::U);
    }
    if behavior.has_mut_param {
        caps.rvs_insert_M(Capability::M);
    }
    if behavior.has_static_mut_ref {
        caps.rvs_insert_M(Capability::S);
        caps.rvs_insert_M(Capability::U);
    } else if behavior.has_static_ref {
        caps.rvs_insert_M(Capability::S);
    }
    if behavior.has_thread_local_ref {
        caps.rvs_insert_M(Capability::S);
        caps.rvs_insert_M(Capability::T);
    }
    caps
}

/// Format an error message for unknown callees (functions with no capability data).
fn rvs_format_unknown_callees(
    unknown: &BTreeMap<String, BTreeSet<String>>,
    header: &str,
) -> String {
    let mut msg = String::from(header);
    for (callee, callers) in unknown {
        msg.push_str(&format!("  {callee}=\n"));
        for caller in callers.iter().take(3) {
            msg.push_str(&format!("    called by: {caller}\n"));
        }
        if callers.len() > 3 {
            msg.push_str(&format!("    ... and {} more\n", callers.len() - 3));
        }
    }
    msg
}

/// Generate trait-method aliases (e.g. `std::io::Read::read`) from impl-method keys
/// (e.g. `std::fs::read@std::io::Read`) by majority-vote resolution.
fn rvs_generate_trait_aliases_MP(
    inferred: &BTreeMap<String, CapabilitySet>,
    impl_index: &HashMap<String, Vec<String>>,
    callgraph: &BTreeMap<String, ParsedFnBehavior>,
) -> BTreeMap<String, CapabilitySet> {
    let mut aliases = BTreeMap::new();
    let mut seen = HashSet::new();
    for key in inferred.keys() {
        if let Some(at_pos) = key.find('@') {
            let trait_path = &key[at_pos + 1..];
            let method_full = &key[..at_pos];
            if let Some(method_name) = method_full.rsplit("::").next() {
                let alias = format!("{trait_path}::{method_name}");
                if seen.insert(alias.clone()) {
                    if let Some(voted) =
                        rvs_resolve_impl_union_M(&alias, impl_index, inferred, callgraph)
                    {
                        aliases.insert(alias, voted);
                    }
                }
            }
        }
    }
    aliases
}

/// Convert a `CapabilitySet` to its uppercase letter string (e.g. {B,I} → "BI").
fn rvs_caps_to_string(caps: &CapabilitySet) -> String {
    caps.rvs_iter().map(|c| c.rvs_as_char()).collect()
}

/// Validate that `path` is a directory, returning an error message if not.
fn rvs_ensure_project_dir_BS(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }
    Ok(())
}

fn rvs_infer_caps_M(
    callgraph: &BTreeMap<String, ParsedFnBehavior>,
    seed: &capsmap::CapsMap,
) -> BTreeMap<String, CapabilitySet> {
    let mut inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

    for (func, behavior) in callgraph {
        if let Some(caps) = seed.rvs_lookup(func) {
            inferred.insert(func.clone(), caps.clone());
        } else {
            inferred.insert(func.clone(), rvs_infer_signature_caps(behavior));
        }
    }
    for behavior in callgraph.values() {
        for callee in &behavior.calls {
            if !inferred.contains_key(callee)
                && let Some(caps) = seed.rvs_lookup(callee)
            {
                inferred.insert(callee.clone(), caps.clone());
            }
        }
    }

    let impl_index = rvs_build_impl_index(&callgraph);

    // Propagation: iterate until fixpoint (no new caps are added).
    //
    // We use a bounded fixpoint loop (max 16 iterations, enough for
    // the deepest call chains in std — typically <10 hops).
    // Caps grow monotonically, so convergence is guaranteed.
    //
    // Seed entries are frozen — their caps are fixed and cannot be changed
    // by propagation. Skip them during the propagation loop.
    let max_iterations = 16;
    for _iteration in 0..max_iterations {
        let mut changed = false;
        for (func, behavior) in callgraph {
            // Skip seed entries — they are frozen.
            if seed.rvs_lookup(func).is_some() {
                continue;
            }
            // Port trait methods are frozen at {P} — no propagation changes them.
            if behavior.is_port_method {
                continue;
            }
            let mut combined = inferred
                .get(func)
                .cloned()
                .unwrap_or_else(CapabilitySet::rvs_new);
            for callee in &behavior.calls {
                // Try direct lookup first (exact match in inferred or seed).
                let callee_caps = inferred
                    .get(callee)
                    .or_else(|| seed.rvs_lookup(callee))
                    .cloned();

                // If not found and callee doesn't contain @ (i.e. it's a
                // trait method definition, not an impl method), try impl-union.
                let callee_caps = callee_caps.or_else(|| {
                    if !callee.contains('@') {
                        rvs_resolve_impl_union_M(callee, &impl_index, &inferred, &callgraph)
                    } else {
                        None
                    }
                });
                if let Some(cc) = callee_caps {
                    for cap in cc.rvs_iter() {
                        // A, M, U are never propagated (signature-only capabilities).
                        // They are inferred from the function's own signature, not
                        // from what it calls.
                        if matches!(cap, Capability::A | Capability::M | Capability::U) {
                            continue;
                        }
                        if !combined.rvs_contains(cap) {
                            combined.rvs_insert_M(cap);
                            changed = true;
                        }
                    }
                }
            }
            inferred.insert(func.clone(), combined);
        }
        if !changed {
            break;
        }
    }
    inferred
}

/// Resolve a trait method callee by taking the union of all impl methods
/// that implement the same trait.
///
/// Callee format: `std::io::Read::read` → method=`read`, trait=`Read`
/// We look up `read@Read` in the impl_index to find all impl methods like
/// `std::fs::impl::read@Read`, `std::io::cursor::impl::read@Read`, etc.
///
/// Port trait methods (is_port_method) always resolve to {P} — no voting.
/// For non-Port traits: A and U are never propagated (signature-only).
/// All other caps (B, I, M, S, T) are eligible for ≥50% majority vote.
fn rvs_resolve_impl_union_M(
    callee: &str,
    impl_index: &HashMap<String, Vec<String>>,
    inferred: &BTreeMap<String, CapabilitySet>,
    callgraph: &BTreeMap<String, ParsedFnBehavior>,
) -> Option<CapabilitySet> {
    // Callee is like "std::io::Read::read"
    // Extract method name (last ::-segment) and trait path (everything before)
    let Some((trait_path, method)) = callee.rsplit_once("::") else {
        return None;
    };

    // Look up "method@trait_path" in the impl_index
    // e.g. "read@std::io::Read"
    let lookup_key = format!("{method}@{trait_path}");
    let impl_keys = impl_index.get(&lookup_key)?;

    // Port trait short-circuit: if any impl method is a Port method,
    // the trait method resolves to {P} only — no voting.
    for key in impl_keys {
        if let Some(behavior) = callgraph.get(key) {
            if behavior.is_port_method {
                let mut caps = CapabilitySet::rvs_new();
                caps.rvs_insert_M(Capability::P);
                return Some(caps);
            }
        }
    }

    // Majority-vote: a capability is propagated if it appears in ≥50% of impls.
    // This avoids rare impls (e.g. RwLock::read having T) polluting the trait.
    // A and U are never propagated (detected from function signature only).
    // S and T are eligible for majority vote — if most impls have them,
    // they should propagate. The vote naturally filters rare caps.
    let mut cap_counts: HashMap<Capability, usize> = HashMap::new();
    let mut total = 0usize;
    for key in impl_keys {
        if let Some(caps) = inferred.get(key) {
            total += 1;
            for cap in caps.rvs_iter() {
                if !matches!(cap, Capability::A | Capability::U) {
                    *cap_counts.entry(cap).or_default() += 1;
                }
            }
        }
    }

    if total == 0 {
        return None;
    }

    let threshold = total.div_ceil(2); // ≥50%
    let mut union = CapabilitySet::rvs_new();
    for (cap, count) in &cap_counts {
        if *count >= threshold {
            union.rvs_insert_M(*cap);
        }
    }

    // Return the union, even if empty. An empty result means "this trait
    // method is known to be pure" (all impls are pure), which is different
    // from None ("unknown — no impls found at all").
    Some(union)
}

fn rvs_format_capsmap(caps: &BTreeMap<String, CapabilitySet>) -> String {
    let mut lines: Vec<String> = caps
        .iter()
        .map(|(name, cs)| {
            let caps_str = rvs_caps_to_string(cs);
            if caps_str.is_empty() {
                format!("{name}=")
            } else {
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{name}={caps_str} # {desc}")
            }
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

/// Collect direct external dependencies called by project functions.
///
/// For each function in `callgraph` that belongs to `crate_name`, looks at its
/// callees. If a callee is external (doesn't start with the crate prefix) and
/// not in the seed capsmap, collects it.
///
/// **Errors on unknown callees** (not in `inferred`) — the caller must add
/// them to seed or ext before proceeding.
///
/// Returns (known_deps, unknown_callees) where unknown_callees maps each
/// unknown callee to a set of callers that reference it.
fn rvs_collect_direct_external_deps(
    callgraph: &BTreeMap<String, ParsedFnBehavior>,
    crate_name: &str,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<String, CapabilitySet>,
    impl_index: &HashMap<String, Vec<String>>,
) -> (
    BTreeMap<String, CapabilitySet>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let crate_prefix = format!("{crate_name}::");
    let mut known: BTreeMap<String, CapabilitySet> = BTreeMap::new();
    let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (func, behavior) in callgraph {
        if !func.starts_with(&crate_prefix) {
            continue;
        }
        for callee in &behavior.calls {
            if callee.starts_with(&crate_prefix) {
                continue;
            }
            // Skip any callee already covered by loaded caps (std/deps/seed/ext)
            if seed.rvs_lookup(callee).is_some() {
                continue;
            }
            if let Some(caps) = inferred.get(callee) {
                known.entry(callee.clone()).or_insert_with(|| caps.clone());
            } else if let Some(caps) =
                rvs_resolve_impl_union_M(callee, impl_index, inferred, callgraph)
            {
                known.entry(callee.clone()).or_insert(caps);
            } else {
                unknown
                    .entry(callee.clone())
                    .or_default()
                    .insert(func.clone());
            }
        }
    }
    (known, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    // ─── rvs_build_report ───────────────────────────────────────────────

    #[test]
    fn test_20260607_report_empty() {
        let entries = vec![];
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_empty", &output);
        assert_eq!(report.total_fn_count, 0);
        assert_eq!(report.total_line_count, 0);
    }

    #[test]
    fn test_20260607_report_pure_only() {
        let entries = vec![FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            line_count: 10,
            is_test: false,
            allows_dead_code: false,
        }];
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_pure_only", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 1);
        assert_eq!(report.ok_fn_count, 1);
    }

    #[test]
    fn test_20260607_report_mixed() {
        let entries = vec![
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 100,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_from_validated("BI"),
                line_count: 50,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_from_validated("M"),
                line_count: 30,
                is_test: false,
                allows_dead_code: false,
            },
        ];
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_mixed", &output);
        assert_eq!(report.total_fn_count, 3);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 2);
        assert_eq!(report.ok_fn_count, 2);
        assert_eq!(report.total_line_count, 180);
    }

    #[test]
    fn test_20260607_report_skips_test_and_dead_code() {
        let entries = vec![
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 10,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 20,
                is_test: true,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 30,
                is_test: false,
                allows_dead_code: true,
            },
        ];
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_skips_test_and_dead_code", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.total_line_count, 10);
    }

    // ─── JSON parsing ───────────────────────────────────────────────────

    #[test]
    fn test_20260608_json_parse_empty() {
        let entries = rvs_parse_report_json_S("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_20260608_json_parse_single_pure() {
        let json =
            r#"[{"name":"rvs_add","caps":"","lines":5,"is_test":false,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].capabilities.rvs_is_empty());
        assert_eq!(entries[0].line_count, 5);
        assert!(!entries[0].is_test);
    }

    #[test]
    fn test_20260608_json_parse_with_caps() {
        let json = r#"[{"name":"rvs_write_BI","caps":"BI","lines":10,"is_test":false,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].capabilities.rvs_contains(Capability::B));
        assert!(entries[0].capabilities.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260608_json_parse_test_fn() {
        let json = r#"[{"name":"test_20260608_foo","caps":"S","lines":3,"is_test":true,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_test);
    }

    // ─── setup functions ────────────────────────────────────────────────

    #[test]
    fn test_20260607_setup_inject_clippy_empty() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        rvs_snapshot_BIS(
            "test_20260607_setup_inject_clippy_empty",
            &format!("count: {count}\n{result}"),
        );
        assert_eq!(count, setup::CLIPPY_LINTS.len());
        assert!(result.contains("[lints.clippy]"));
    }

    #[test]
    fn test_20260607_setup_inject_clippy_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, c1) = rvs_inject_clippy_lints_M(input);
        let (second, c2) = rvs_inject_clippy_lints_M(&first);
        assert!(c1 > 0);
        assert_eq!(c2, 0);
        assert_eq!(first, second);
    }

    #[test]
    fn test_20260607_setup_inject_clippy_preserves() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        assert!(result.contains("string_slice = \"deny\""));
        assert_eq!(count, setup::CLIPPY_LINTS.len() - 1);
    }

    // ─── rvs_infer_caps_M ────────────────────────────────────────────────

    /// Helper: build a default `ParsedFnBehavior` with all flags false and no calls.
    fn rvs_make_behavior() -> ParsedFnBehavior {
        ParsedFnBehavior {
            calls: BTreeSet::new(),
            has_async: false,
            is_unsafe_fn: false,
            has_mut_param: false,
            has_static_ref: false,
            has_static_mut_ref: false,
            has_thread_local_ref: false,
            is_trait_impl: false,
            is_test: false,
            is_port_method: false,
        }
    }

    #[test]
    fn test_20260609_infer_caps_empty_callgraph() {
        let callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        rvs_snapshot_BIS(
            "test_20260609_infer_caps_empty_callgraph",
            &format!("{result:?}"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_20260613_seed_freeze_prevents_propagation() {
        // Seed entry should freeze caps — even if callee is P, the seed entry
        // (empty) should prevent P from appearing on the function.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();

        let mut cap_overflow = rvs_make_behavior();
        cap_overflow.calls.insert("core::panicking::panic".into());
        callgraph.insert("alloc::raw_vec::capacity_overflow".into(), cap_overflow);

        // panic: true panic
        let mut panic = rvs_make_behavior();
        callgraph.insert("core::panicking::panic".into(), panic);

        // handle_error: calls capacity_overflow
        let mut handle_error = rvs_make_behavior();
        handle_error
            .calls
            .insert("alloc::raw_vec::capacity_overflow".into());
        callgraph.insert("alloc::raw_vec::handle_error".into(), handle_error);

        // Seed freezes capacity_overflow to empty (no P)
        let seed = capsmap::CapsMap::rvs_parse(
            "alloc::raw_vec::capacity_overflow=\nalloc::raw_vec::handle_error=\n",
        )
        .unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);

        let cap_caps = result.get("alloc::raw_vec::capacity_overflow");
        assert!(
            cap_caps.is_none_or(|c| c.rvs_is_empty()),
            "capacity_overflow should be frozen to empty by seed, got: {cap_caps:?}"
        );

        let handle_caps = result.get("alloc::raw_vec::handle_error");
        assert!(
            handle_caps.is_none_or(|c| c.rvs_is_empty()),
            "handle_error should be frozen to empty by seed, got: {handle_caps:?}"
        );
    }

    #[test]
    fn test_20260609_infer_caps_single_pure() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        callgraph.insert("my_crate::rvs_add".into(), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_pure", &output);
        // Pure function: no caps inferred, so it should be absent from the result
        assert!(
            result
                .get("my_crate::rvs_add")
                .is_none_or(|c| c.rvs_is_empty())
        );
    }

    #[test]
    fn test_20260609_infer_caps_single_panic() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_divide".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_panic", &output);
        let caps = result
            .get("my_crate::rvs_divide")
            .expect("should have entry");
        assert!(caps.rvs_is_empty());
        assert_eq!(caps.rvs_len(), 0);
    }

    #[test]
    fn test_20260609_infer_caps_single_static_ref() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.has_static_ref = true;
        callgraph.insert("my_crate::rvs_get_env_S".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_static_ref", &output);
        let caps = result
            .get("my_crate::rvs_get_env_S")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::S));
        assert_eq!(caps.rvs_len(), 1);
    }

    #[test]
    fn test_20260609_infer_caps_single_unsafe_block() {
        // Unsafe blocks no longer trigger U — only `unsafe fn` declarations do.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_ffi_call".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        // No U — unsafe block alone does not give U.
        // Function is still in inferred (with empty caps).
        let caps = result.get("my_crate::rvs_ffi_call");
        assert!(caps.is_some());
        assert!(caps.unwrap().rvs_is_empty());
    }

    #[test]
    fn test_20260609_infer_caps_propagation_caller_gets_io() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut caller_behavior = rvs_make_behavior();
        caller_behavior
            .calls
            .insert("std::fs::read_to_string".into());
        callgraph.insert("my_crate::rvs_process".into(), caller_behavior);
        callgraph.insert("std::fs::read_to_string".into(), rvs_make_behavior());

        // Seed says read_to_string has BI
        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS(
            "test_20260609_infer_caps_propagation_caller_gets_io",
            &output,
        );

        // Caller should inherit BI from callee
        let caller_caps = result
            .get("my_crate::rvs_process")
            .expect("caller should have entry");
        assert!(caller_caps.rvs_contains(Capability::B));
        assert!(caller_caps.rvs_contains(Capability::I));
        assert_eq!(caller_caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260609_infer_caps_propagation_chain() {
        // A calls B, B calls C. C has S in seed. A should get S via B.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut a_behavior = rvs_make_behavior();
        a_behavior.calls.insert("my_crate::B".into());
        callgraph.insert("my_crate::A".into(), a_behavior);

        let mut b_behavior = rvs_make_behavior();
        b_behavior.calls.insert("my_crate::C".into());
        callgraph.insert("my_crate::B".into(), b_behavior);

        callgraph.insert("my_crate::C".into(), rvs_make_behavior());

        let seed = capsmap::CapsMap::rvs_parse("my_crate::C=S").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_propagation_chain", &output);

        let a_caps = result.get("my_crate::A").expect("A should have entry");
        let b_caps = result.get("my_crate::B").expect("B should have entry");
        assert!(a_caps.rvs_contains(Capability::S));
        assert!(b_caps.rvs_contains(Capability::S));
    }

    #[test]
    fn test_20260609_infer_caps_cycle_self_recursive() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("my_crate::rvs_loop".into());
        callgraph.insert("my_crate::rvs_loop".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_cycle_self_recursive", &output);

        // Self-recursive with no caps: stays empty
        assert!(
            result
                .get("my_crate::rvs_loop")
                .is_none_or(|c| c.rvs_is_empty())
        );
    }

    #[test]
    fn test_20260609_infer_caps_cycle_mutual_recursion() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut a_behavior = rvs_make_behavior();
        a_behavior.calls.insert("my_crate::B".into());
        callgraph.insert("my_crate::A".into(), a_behavior);

        let mut b_behavior = rvs_make_behavior();
        b_behavior.calls.insert("my_crate::A".into());
        callgraph.insert("my_crate::B".into(), b_behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_cycle_mutual_recursion", &output);

        // Mutual recursion with no caps: both stay empty
        assert!(result.get("my_crate::A").is_none_or(|c| c.rvs_is_empty()));
        assert!(result.get("my_crate::B").is_none_or(|c| c.rvs_is_empty()));
    }

    #[test]
    fn test_20260609_infer_caps_seed_override() {
        // Seed should win — the inferred result should only have BI, not P.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_read_BI".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("my_crate::rvs_read_BI=BI").unwrap();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_seed_override", &output);

        let caps = result
            .get("my_crate::rvs_read_BI")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(
            !caps.rvs_contains(Capability::T),
            "seed should override behavioral flags"
        );
        assert_eq!(caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260609_infer_caps_rvs_suffix_from_name() {
        // rvs_ function names carry capability suffixes. But rvs_infer_caps_M doesn't
        // parse suffixes from names — it only uses seed + behavioral flags + propagation.
        // However, if the function is in the callgraph with has_async=true, it gets A.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.has_async = true;
        behavior.has_mut_param = true;
        callgraph.insert("my_crate::rvs_write_db_ABM".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_rvs_suffix_from_name", &output);

        let caps = result
            .get("my_crate::rvs_write_db_ABM")
            .expect("should have entry");
        // A from has_async, M from has_mut_param
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::M));
        assert_eq!(caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260613_infer_caps_propagation_from_bimps_callee() {
        // Reproduces a real-world bug: when a caller calls a callee that has
        // BIMPS, but the callee is part of a larger dependency subgraph that
        // contains cycles, the topological sort (Kahn's algorithm) cannot
        // fully order these nodes. The unsorted "cycle" nodes are appended
        // in BTreeMap (alphabetical) key order, which may place the caller
        // BEFORE the callee. This means the callee's propagated caps haven't
        // been computed yet when the caller is processed, so the caller
        // misses B, I, S from the callee.
        //
        // The callgraph mirrors the real std::process::spawn scenario:
        //   caller (std::process::impl::spawn) calls callee
        //   callee (std::sys::process::unix::unix::impl::spawn) calls
        //     a deep callee (in seed with BIS) AND a node in a cycle.
        //   The cycle prevents Kahn's from processing callee normally,
        //   so callee ends up in the "cycle nodes" section appended in
        //   alphabetical order — AFTER the caller.
        //
        // After the fix, the caller should get BIMPS regardless of the
        // ordering, because the propagation should handle cycle nodes
        // correctly (e.g. by iterating until fixpoint or by a different
        // approach).
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();

        // Caller: has M from has_mut_param
        let mut caller_behavior = rvs_make_behavior();
        caller_behavior.has_mut_param = true;
        caller_behavior
            .calls
            .insert("std::sys::process::unix::unix::impl::spawn".into());
        callgraph.insert("std::process::impl::spawn".into(), caller_behavior);

        // Callee: calls a seed function with BIS + a node in a cycle
        let mut callee_behavior = rvs_make_behavior();
        callee_behavior.has_mut_param = true;
        callee_behavior
            .calls
            .insert("std::sys::pal::unix::kernel_copy::rvs_write".into());
        // This creates a cycle: cycle_a -> cycle_b -> cycle_a
        callee_behavior.calls.insert("std::sys::cycle_a".into());
        callgraph.insert(
            "std::sys::process::unix::unix::impl::spawn".into(),
            callee_behavior,
        );

        // Cycle nodes (block Kahn's from processing callee)
        let mut cycle_a = rvs_make_behavior();
        cycle_a.calls.insert("std::sys::cycle_b".into());
        callgraph.insert("std::sys::cycle_a".into(), cycle_a);

        let mut cycle_b = rvs_make_behavior();
        cycle_b.calls.insert("std::sys::cycle_a".into());
        callgraph.insert("std::sys::cycle_b".into(), cycle_b);

        // Deep callee: leaf function in seed with BIS
        let seed =
            capsmap::CapsMap::rvs_parse("std::sys::pal::unix::kernel_copy::rvs_write=BIS").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS(
            "test_20260613_infer_caps_propagation_from_bimps_callee",
            &output,
        );

        // The callee should have BIMPS (BIS from seed callee + MP from flags)
        let callee_caps = result
            .get("std::sys::process::unix::unix::impl::spawn")
            .expect("callee should have entry");
        assert!(
            callee_caps.rvs_contains(Capability::B),
            "callee should have B from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::I),
            "callee should have I from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::M),
            "callee should have M from has_mut_param"
        );
        assert!(
            callee_caps.rvs_contains(Capability::S),
            "callee should have S from deep callee"
        );

        // The caller should also have BIMPS (BIS propagated from callee + M from flags)
        let caller_caps = result
            .get("std::process::impl::spawn")
            .expect("caller should have entry");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "caller should have B propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::I),
            "caller should have I propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::M),
            "caller should have M from has_mut_param"
        );
        assert!(
            caller_caps.rvs_contains(Capability::S),
            "caller should have S propagated from callee"
        );
    }

    #[test]
    fn test_20260613_impl_union_majority_vote() {
        // Three impls of Read::read:
        //   File::read   → BI (real I/O, from libc::read seed)
        //   Cursor::read → (empty, in-memory)
        //   &[u8]::read  → (empty, pure read)
        //
        // Majority vote (≥50% = ≥2 out of 3):
        //   B: 1/3 → ❌ don't propagate
        //   I: 1/3 → ❌ don't propagate
        //   M: 3/3 → ❌ don't propagate (inferred from signature only)
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert("std::io::Read::read".into());
        callgraph.insert("my_crate::rvs_copy".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.has_mut_param = true;
        file_read.calls.insert("libc::unix::read".into());
        callgraph.insert("std::fs::read@std::io::Read".into(), file_read);

        let mut cursor_read = rvs_make_behavior();
        cursor_read.has_mut_param = true;
        callgraph.insert("std::io::cursor::read@std::io::Read".into(), cursor_read);

        let mut slice_read = rvs_make_behavior();
        slice_read.has_mut_param = true;
        callgraph.insert("std::io::impls::read@std::io::Read".into(), slice_read);

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result.get("my_crate::rvs_copy").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M: not propagated"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::B),
            "B: 1/3 = minority, should not propagate"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::I),
            "I: 1/3 = minority, should not propagate"
        );
    }

    #[test]
    fn test_20260614_m_not_propagated_from_direct_call() {
        // M is a signature-only capability — it is NOT propagated through calls,
        // just like A and U. A function gets M only if it has &mut parameters
        // in its own signature. The call rule in check also exempts A/M/U.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();

        // caller has no &mut params of its own
        let mut caller = rvs_make_behavior();
        caller.has_async = true;
        caller.calls.insert("my_crate::sort_inplace".into());
        callgraph.insert("my_crate::handle".into(), caller);

        // callee has &mut param → gets M from signature
        let mut callee = rvs_make_behavior();
        callee.has_mut_param = true;
        callgraph.insert("my_crate::sort_inplace".into(), callee);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result.get("my_crate::handle").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M should NOT propagate — signature-only capability"
        );
        assert!(caller_caps.rvs_contains(Capability::A), "A from has_async");
    }

    #[test]
    fn test_20260613_impl_union_no_cross_trait() {
        // Two traits with same method name "read":
        //   std::io::Read::read   → File impl has BIMP
        //   std::sync::RwLock     → read impl has M (not a file read!)
        //
        // The caller calls Read::read. It should NOT pick up caps from
        // RwLock's read method, because the @TraitPath differs.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert("std::io::Read::read".into());
        callgraph.insert("my_crate::rvs_read_data".into(), caller);

        // Read impl: has B from libc
        let mut file_read = rvs_make_behavior();
        file_read.calls.insert("libc::unix::read".into());
        callgraph.insert("std::fs::read@std::io::Read".into(), file_read);

        // RwLock impl: completely unrelated
        let mut rwlock_read = rvs_make_behavior();
        rwlock_read.has_mut_param = true;
        callgraph.insert(
            "std::sync::rwlock::read@std::sync::RwLock".into(),
            rwlock_read,
        );

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();
        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result
            .get("my_crate::rvs_read_data")
            .expect("caller exists");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "should get B from Read::read impl"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "should NOT get M from RwLock::read (different trait)"
        );
    }

    // ─── rvs_format_capsmap ────────────────────────────────────────────

    #[test]
    fn test_20260609_format_capsmap_empty() {
        let map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_empty", &output);
        assert_eq!(output, "\n");
    }

    #[test]
    fn test_20260609_format_capsmap_single_entry() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_single_entry", &output);
        assert_eq!(output, "std::fs::read=BI # Blocking IO\n");
    }

    #[test]
    fn test_20260609_format_capsmap_multiple_sorted() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::process::exit".into(),
            CapabilitySet::rvs_from_validated("S"),
        );
        map.insert("HashMap::new".into(), CapabilitySet::rvs_new());
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_multiple_sorted", &output);
        // BTreeMap is already sorted, so output should be alphabetical by key
        let lines: Vec<&str> = output.trim_end().lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("HashMap::new"));
        assert!(lines[1].starts_with("std::fs::read"));
        assert!(lines[2].starts_with("std::process::exit"));
    }

    // ─── rvs_parse_callgraph_S ────────────────────────────────────────────

    #[test]
    fn test_20260609_parse_callgraph_valid_json() {
        let json = r#"{
            "my_crate::rvs_add": {
                "calls": ["my_crate::rvs_helper"],
                "has_async": false,
                                "is_unsafe_fn": false,
                "has_mut_param": false,
                                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            },
            "my_crate::rvs_write_BI": {
                "calls": ["std::fs::write"],
                "has_async": false,
                                "is_unsafe_fn": false,
                "has_mut_param": false,
                                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#;
        let result = rvs_parse_callgraph_S(json).unwrap();
        let output = format!("{result:?}");
        rvs_snapshot_BIS("test_20260609_parse_callgraph_valid_json", &output);
        assert_eq!(result.len(), 2);

        let add_behavior = result
            .get("my_crate::rvs_add")
            .expect("should find rvs_add");
        assert!(add_behavior.calls.contains("my_crate::rvs_helper"));

        let write_behavior = result
            .get("my_crate::rvs_write_BI")
            .expect("should find rvs_write_BI");
        assert!(write_behavior.calls.contains("std::fs::write"));
    }

    #[test]
    fn test_20260609_parse_callgraph_invalid_json() {
        let json = "this is not json at all";
        let result = rvs_parse_callgraph_S(json);
        rvs_snapshot_BIS(
            "test_20260609_parse_callgraph_invalid_json",
            &format!("{result:?}"),
        );
        assert!(result.is_err());
    }

    // ─── rvs_collect_direct_external_deps ────────────────────────────────

    #[test]
    fn test_20260611_unknown_callee_reported_as_error() {
        // infer-capsmap must report unknown external callees as errors,
        // not silently skip them or mark them as pure.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior
            .calls
            .insert("some_external_crate::unknown_fn".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            "my_crate",
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(known.is_empty());
        assert!(
            unknown.contains_key("some_external_crate::unknown_fn"),
            "unknown callee must be reported as error"
        );
        assert_eq!(unknown.len(), 1);
        assert!(unknown["some_external_crate::unknown_fn"].contains("my_crate::caller"));
    }

    #[test]
    fn test_20260611_inferred_callee_is_known() {
        // If a callee IS in the inferred map, it goes to known, not unknown.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior
            .calls
            .insert("some_external_crate::known_fn".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        inferred.insert(
            "some_external_crate::known_fn".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            "my_crate",
            &seed,
            &inferred,
            &HashMap::new(),
        );

        let caps = known
            .get("some_external_crate::known_fn")
            .expect("should have entry in known");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_seed_callee_is_skipped() {
        // If a callee is already in the seed, it's neither known nor unknown.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::fs::write".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("std::fs::write=BI").unwrap();
        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            "my_crate",
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(!known.contains_key("std::fs::write"));
        assert!(!unknown.contains_key("std::fs::write"));
    }

    #[test]
    fn test_20260613_inherent_impl_no_collision() {
        // Regression test for def_path collision:
        // Before the fix, rvs_def_path dropped the type name from
        // inherent impl blocks, causing SystemTime::now() and
        // Instant::now() to both appear as "std::time::now" in the
        // callgraph.  The seed entry "std::time::SystemTime::now=S"
        // could not match "std::time::now".
        //
        // After the fix, inherent impl methods include the type name:
        //   SystemTime::now() → "std::time::SystemTime::now"
        //   Instant::now()    → "std::time::Instant::now"
        //
        // This test verifies that seed entries with the full
        // Type::method path are correctly matched when they appear as
        // callees in the callgraph.

        // Simulate a caller that calls SystemTime::now (now with the
        // correct def_path that includes the type name).
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::time::SystemTime::now".into());
        callgraph.insert("my_crate::rvs_get_time".into(), behavior);

        // Seed entry uses the full path with the type name.
        let seed = capsmap::CapsMap::rvs_parse("std::time::SystemTime::now=S").unwrap();

        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            "my_crate",
            &seed,
            &inferred,
            &HashMap::new(),
        );

        // The callee should be found in seed (not known, not unknown).
        assert!(
            !unknown.contains_key("std::time::SystemTime::now"),
            "seed entry should match the full def_path"
        );
        assert!(!known.contains_key("std::time::SystemTime::now"));
    }
}
