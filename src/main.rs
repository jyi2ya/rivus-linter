#![feature(rustc_private)]
#![expect(
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
use setup::{rvs_inject_clippy_lints_M, rvs_inject_spawn_capsmap_M};

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
        Some(Commands::Usage) => {
            print!("{RIVUS_MANUAL}");
        }
    }
    ExitCode::SUCCESS
}

// ─── Check subcommand ────────────────────────────────────────────────────

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
fn rvs_run_cargo_check_BIMPS(capsmap: Option<PathBuf>, extra_args: Vec<String>) -> Result<(), i32> {
    let self_path = env::current_exe().expect("current executable path invalid");
    let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.env("RUSTC_WORKSPACE_WRAPPER", &self_path)
        .env("RIVUS_ENABLED", "1");

    // Resolve capsmap: user-provided > inferred > built-in caps/ directory
    let resolved_capsmap = capsmap
        .and_then(|p| {
            if p.exists() {
                Some(p)
            } else {
                eprintln!("Warning: capsmap '{}' not found, ignoring", p.display());
                None
            }
        })
        .or_else(|| {
            let inferred = PathBuf::from("target/rivus-inferred-capsmap.txt");
            if inferred.exists() {
                Some(inferred)
            } else {
                None
            }
        });

    if let Some(ref p) = resolved_capsmap {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            std::env::current_dir()
                .expect("current dir invalid")
                .join(p)
        };
        cmd.env("RIVUS_CAPSMAP", abs);
    } else {
        // No user capsmap — try the project's caps/ directory first,
        // then fall back to the linter's built-in caps/ directory.
        let project_caps = std::env::current_dir().ok().map(|cwd| cwd.join("caps"));
        let project_caps_dir = project_caps.as_ref().filter(|p| p.is_dir());

        let built_in_caps = self_path.parent().and_then(|exe_dir| {
            exe_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|root| root.join("caps"))
        });
        let built_in_caps_dir = built_in_caps.as_ref().filter(|p| p.is_dir());

        if let Some(dir) = project_caps_dir.or(built_in_caps_dir) {
            cmd.env("RIVUS_CAPSMAP", dir);
        }
    }

    cmd.arg("check").arg("--tests").args(&extra_args);
    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if exit_status.success() {
        Ok(())
    } else {
        Err(exit_status.code().unwrap_or(-1))
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
    has_unsafe_block: bool,
    is_unsafe_fn: bool,
    has_mut_param: bool,
    has_panic: bool,
    has_static_ref: bool,
    has_static_mut_ref: bool,
    has_thread_local_ref: bool,
    is_trait_impl: bool,
}

fn rvs_build_report(entries: &[FnEntry]) -> Report {
    let mut by_capability: BTreeMap<Capability, CapStats> = BTreeMap::new();
    let mut pure_fn_count = 0usize;
    let mut pure_line_count = 0usize;
    let mut good_fn_count = 0usize;
    let mut good_line_count = 0usize;
    let mut total_fn_count = 0usize;
    let mut total_line_count = 0usize;
    let good_allowed = CapabilitySet::rvs_from_good_caps();

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
    }

    Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        total_fn_count,
        total_line_count,
    }
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_report_BIMPS(path: &Path) {
    let report_dir = path.join("target").join("rivus-report");
    let build_dir = path.join("target").join("rivus-report-build");
    let self_path = env::current_exe().expect("current executable path invalid");
    let abs_report_dir = std::env::current_dir()
        .expect("current dir invalid")
        .join(&report_dir);
    rvs_clean_dir(&report_dir);
    rvs_clean_dir(&build_dir);
    let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(path)
        .env("RUSTC_WORKSPACE_WRAPPER", self_path)
        .env("RIVUS_ENABLED", "1")
        .env("RIVUS_REPORT", "1")
        .env("RIVUS_REPORT_DIR", abs_report_dir)
        .arg("check")
        .arg("--target-dir")
        .arg(&build_dir);
    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        process::exit(exit_status.code().unwrap_or(-1));
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
            let entries = rvs_parse_report_json(&json_str).unwrap_or_else(|e| {
                eprintln!("Error: parsing {}: {e}", p.display());
                process::exit(2);
            });
            all_entries.extend(entries);
        }
    }
    let report = rvs_build_report(&all_entries);
    print!("{report}");
}

fn rvs_parse_report_json(json: &str) -> Result<Vec<FnEntry>, String> {
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
    if !path.is_dir() {
        eprintln!("Error: '{}' is not a directory", path.display());
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

    let caps_dir = path.join("caps");
    let seed_path = caps_dir.join("seed");
    if caps_dir.is_dir() && seed_path.exists() {
        let seed_content = std::fs::read_to_string(&seed_path).unwrap_or_else(|e| {
            eprintln!("Error: cannot read '{}': {e}", seed_path.display());
            process::exit(2);
        });
        let (new_seed, spawn_count) = rvs_inject_spawn_capsmap_M(&seed_content);
        if spawn_count > 0 {
            std::fs::write(&seed_path, &new_seed).unwrap_or_else(|e| {
                eprintln!("Error: cannot write '{}': {e}", seed_path.display());
                process::exit(2);
            });
            println!(
                "Injected {spawn_count} spawn capsmap entries into {}",
                seed_path.display()
            );
        } else {
            println!(
                "All spawn capsmap entries already present in {}",
                seed_path.display()
            );
        }
    } else {
        // Legacy: inject into capsmap.txt
        let capsmap_path = path.join("capsmap.txt");
        if capsmap_path.exists() {
            let capsmap_content = std::fs::read_to_string(&capsmap_path).unwrap_or_else(|e| {
                eprintln!("Error: cannot read '{}': {e}", capsmap_path.display());
                process::exit(2);
            });
            let (new_capsmap, spawn_count) = rvs_inject_spawn_capsmap_M(&capsmap_content);
            if spawn_count > 0 {
                std::fs::write(&capsmap_path, &new_capsmap).unwrap_or_else(|e| {
                    eprintln!("Error: cannot write '{}': {e}", capsmap_path.display());
                    process::exit(2);
                });
                println!(
                    "Injected {spawn_count} spawn capsmap entries into {}",
                    capsmap_path.display()
                );
            } else {
                println!(
                    "All spawn capsmap entries already present in {}",
                    capsmap_path.display()
                );
            }
        }
    }
}

// ─── InferCapsmap subcommand ─────────────────────────────────────────────

fn rvs_load_seed_capsmap_BIMS(path: &Path, seed_path: &Path) -> capsmap::CapsMap {
    // Try loading as directory first (caps/ dir), then as single file
    if seed_path.is_dir() {
        capsmap::CapsMap::rvs_load_from_dir_BIMS(seed_path).unwrap_or_else(|e| {
            eprintln!("warning: {}: {e}", seed_path.display());
            capsmap::CapsMap::rvs_new()
        })
    } else if seed_path.is_file() {
        let content = std::fs::read_to_string(seed_path).unwrap_or_else(|e| {
            eprintln!("warning: {}: {e}", seed_path.display());
            String::new()
        });
        capsmap::CapsMap::rvs_parse(&content).unwrap_or_else(|e| {
            eprintln!("warning: {}: {e}", seed_path.display());
            capsmap::CapsMap::rvs_new()
        })
    } else {
        // Try path.join("caps") as directory
        let caps_dir = path.join("caps");
        if caps_dir.is_dir() {
            capsmap::CapsMap::rvs_load_from_dir_BIMS(&caps_dir).unwrap_or_else(|e| {
                eprintln!("warning: {}: {e}", caps_dir.display());
                capsmap::CapsMap::rvs_new()
            })
        } else {
            capsmap::CapsMap::rvs_new()
        }
    }
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
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let name = rest.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Ok(name.replace('-', "_"));
                }
            }
        }
    }
    Err("Cargo.toml missing [package].name".into())
}

fn rvs_clean_dir(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_cargo_check_for_callgraph_BIMPS(
    path: &Path,
    extra_env: Vec<(&str, PathBuf)>,
) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let cg_dir = path.join("target").join("rivus-callgraph");
    let cg_target_dir = path.join("target").join("rivus-build");

    rvs_clean_dir(&cg_dir);
    rvs_clean_dir(&cg_target_dir);

    let self_path = env::current_exe().expect("current executable path invalid");
    let abs_cg_dir = std::env::current_dir()
        .expect("current dir invalid")
        .join(&cg_dir);

    let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(path)
        .env("RUSTC_WRAPPER", self_path)
        .env("RIVUS_ENABLED", "1")
        .env("RIVUS_CALLGRAPH", "1")
        .env("RIVUS_CALLGRAPH_DIR", &abs_cg_dir);
    for (key, val) in &extra_env {
        cmd.env(key, val);
    }
    cmd.arg("check").arg("--target-dir").arg(&cg_target_dir);

    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        return Err("cargo check failed — fix compilation errors first".into());
    }

    rvs_merge_callgraph_dir_BI(&cg_dir)
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }

    let callgraph = rvs_run_cargo_check_for_callgraph_BIMPS(path, vec![])?;

    let seed_path = path.join("capsmap.txt");
    let seed = rvs_load_seed_capsmap_BIMS(path, &seed_path);

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
        if short_name == "main" || short_name == "new" || short_name == "drop" {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_trait_impl) {
            skip_names.insert(short_name.to_string());
            continue;
        }
        let caps_str: String = caps.rvs_iter().map(|c| c.rvs_as_char()).collect();
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
fn rvs_run_infer_capsmap_BIMPS(
    path: &Path,
    seed_capsmap: &Path,
    output: Option<&Path>,
) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }

    let abs_seed = if seed_capsmap.is_absolute() {
        seed_capsmap.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("current dir invalid")
            .join(seed_capsmap)
    };

    let callgraph =
        rvs_run_cargo_check_for_callgraph_BIMPS(path, vec![("RIVUS_CAPSMAP", abs_seed)])?;

    let seed = rvs_load_seed_capsmap_BIMS(path, seed_capsmap);

    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    let all_result = rvs_format_capsmap(&inferred);
    let cache_path = path.join("target").join("rivus-inferred-capsmap.txt");
    std::fs::write(&cache_path, &all_result)
        .map_err(|e| format!("cannot write {}: {e}", cache_path.display()))?;

    let crate_name = rvs_detect_crate_name_BIS(path)?;
    let (direct_external_calls, unknown_callees) =
        rvs_collect_direct_external_deps(&callgraph, &crate_name, &seed, &inferred);

    if !unknown_callees.is_empty() {
        let mut msg = String::from(
            "error: the following external functions have no capability data.\n\
             Add them to caps/seed or caps/ext with the correct capability markers:\n\n",
        );
        for (callee, callers) in &unknown_callees {
            msg.push_str(&format!("  {callee}=\n"));
            for caller in callers.iter().take(3) {
                msg.push_str(&format!("    called by: {caller}\n"));
            }
            if callers.len() > 3 {
                msg.push_str(&format!("    ... and {} more\n", callers.len() - 3));
            }
        }
        return Err(msg);
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

fn rvs_merge_callgraph_dir_BI(cg_dir: &Path) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let mut merged: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
    let cg_entries =
        std::fs::read_dir(cg_dir).map_err(|e| format!("cannot read {}: {e}", cg_dir.display()))?;
    for entry in cg_entries {
        let entry = entry.map_err(|e| format!("readdir error: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let json_str = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let partial = rvs_parse_callgraph(&json_str)?;
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
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(format!("'{}' is not a Cargo project", path.display()));
    }

    let cg_dir = path.join("target").join("rivus-callgraph-std");
    let cg_target_dir = path.join("target").join("rivus-build-std");

    if cg_dir.exists() {
        let _ = std::fs::remove_dir_all(&cg_dir);
    }
    if cg_target_dir.exists() {
        let _ = std::fs::remove_dir_all(&cg_target_dir);
    }

    {
        let self_path = env::current_exe().expect("current executable path invalid");
        let abs_cg_dir = std::env::current_dir()
            .expect("current dir invalid")
            .join(&cg_dir);
        // Use RUSTUP_TOOLCHAIN=nightly instead of `+nightly` arg, because
        // CARGO env var may point to cargo-rivus (our own binary) which
        // doesn't understand +nightly.
        let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cmd.env("RUSTUP_TOOLCHAIN", "nightly")
            .env("RUSTC_WRAPPER", self_path)
            .env("RIVUS_ENABLED", "1")
            .env("RIVUS_CALLGRAPH", "1")
            .env("RIVUS_CALLGRAPH_DIR", &abs_cg_dir)
            .arg("check")
            .arg("-Zbuild-std=std,core,alloc")
            .arg("--manifest-path")
            .arg(&cargo_toml)
            .arg("--target")
            .arg(rvs_host_triple_BIMPS())
            .arg("--target-dir")
            .arg(&cg_target_dir);
        let exit_status = cmd
            .spawn()
            .expect("could not run cargo")
            .wait()
            .expect("failed to wait for cargo?");
        if !exit_status.success() {
            return Err("cargo +nightly check -Zbuild-std failed".into());
        }
    }

    let callgraph = rvs_merge_callgraph_dir_BI(&cg_dir)?;

    let seed = rvs_load_seed_capsmap_BIMS(path, &path.join("caps"));

    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    // Filter to only std/core/alloc functions with non-empty capability sets.
    let crate_name = rvs_detect_crate_name_BIS(path)?;
    let crate_prefix = format!("{crate_name}::");
    let std_crates: &[&str] = &["std::", "core::", "alloc::", "compiler_builtins::"];
    let std_only: BTreeMap<String, CapabilitySet> = inferred
        .iter()
        .filter(|(name, caps)| {
            !name.starts_with(&crate_prefix)
                && !caps.rvs_is_empty()
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
        // Only check std functions that were inferred as pure (empty caps).
        // If they already have caps, they're fine.
        if let Some(caps) = inferred.get(func) {
            if !caps.rvs_is_empty() {
                continue;
            }
        }
        for callee in &behavior.calls {
            if inferred.get(callee).is_some() {
                continue; // callee has known caps (may be empty = truly pure)
            }
            if seed.rvs_lookup(callee).is_some() {
                continue; // callee is in seed
            }
            // callee is unknown — can't determine if it contributes capabilities
            unknown
                .entry(callee.clone())
                .or_default()
                .insert(func.clone());
        }
    }

    if !unknown.is_empty() {
        let mut msg = String::from(
            "error: the following functions are called by std but have no capability data.\n\
             Add them to caps/seed with the correct capability markers:\n\n",
        );
        for (callee, callers) in &unknown {
            msg.push_str(&format!("  {callee}=\n"));
            for caller in callers.iter().take(3) {
                msg.push_str(&format!("    called by: {caller}\n"));
            }
            if callers.len() > 3 {
                msg.push_str(&format!("    ... and {} more\n", callers.len() - 3));
            }
        }
        return Err(msg);
    }

    let result = rvs_format_capsmap(&std_only);
    let default_path = path.join("target").join("rivus-std-capsmap.txt");
    rvs_write_capsmap_result_BIS(&result, &default_path, output, "std capsmap")
}

/// # Panics
///
/// Panics if `rustc -vV` cannot be executed or returns a non-zero exit status.
fn rvs_host_triple_BIMPS() -> String {
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

fn rvs_parse_callgraph(json: &str) -> Result<BTreeMap<String, ParsedFnBehavior>, String> {
    let raw: BTreeMap<String, JsonFnBehavior> =
        serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.into())).collect())
}

#[derive(Debug, Default)]
struct ParsedFnBehavior {
    calls: BTreeSet<String>,
    has_async: bool,
    has_unsafe_block: bool,
    is_unsafe_fn: bool,
    has_mut_param: bool,
    has_panic: bool,
    has_static_ref: bool,
    has_static_mut_ref: bool,
    has_thread_local_ref: bool,
    is_trait_impl: bool,
}

impl ParsedFnBehavior {
    fn rvs_merge_M(&mut self, other: &Self) {
        self.calls.extend(other.calls.iter().cloned());
        self.has_async |= other.has_async;
        self.has_unsafe_block |= other.has_unsafe_block;
        self.is_unsafe_fn |= other.is_unsafe_fn;
        self.has_mut_param |= other.has_mut_param;
        self.has_panic |= other.has_panic;
        self.has_static_ref |= other.has_static_ref;
        self.has_static_mut_ref |= other.has_static_mut_ref;
        self.has_thread_local_ref |= other.has_thread_local_ref;
        self.is_trait_impl |= other.is_trait_impl;
    }
}

impl From<JsonFnBehavior> for ParsedFnBehavior {
    fn from(j: JsonFnBehavior) -> Self {
        Self {
            calls: j.calls,
            has_async: j.has_async,
            has_unsafe_block: j.has_unsafe_block,
            is_unsafe_fn: j.is_unsafe_fn,
            has_mut_param: j.has_mut_param,
            has_panic: j.has_panic,
            has_static_ref: j.has_static_ref,
            has_static_mut_ref: j.has_static_mut_ref,
            has_thread_local_ref: j.has_thread_local_ref,
            is_trait_impl: j.is_trait_impl,
        }
    }
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
            let mut caps = CapabilitySet::rvs_new();
            if behavior.has_async {
                caps.rvs_insert_M(Capability::A);
            }
            if behavior.is_unsafe_fn || behavior.has_unsafe_block {
                caps.rvs_insert_M(Capability::U);
            }
            if behavior.has_mut_param {
                caps.rvs_insert_M(Capability::M);
            }
            if behavior.has_panic {
                caps.rvs_insert_M(Capability::P);
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
            if !caps.rvs_is_empty() {
                inferred.insert(func.clone(), caps);
            }
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

    let mut changed = true;
    while changed {
        changed = false;
        for (caller, behavior) in callgraph {
            let mut combined = inferred
                .get(caller)
                .cloned()
                .unwrap_or_else(CapabilitySet::rvs_new);

            for callee in &behavior.calls {
                let callee_caps = inferred
                    .get(callee)
                    .or_else(|| {
                        let cn = callee.rsplit("::").next().unwrap_or(callee);
                        inferred
                            .iter()
                            .find(|(k, _)| k.ends_with(&format!("::{cn}")))
                            .map(|(_, v)| v)
                    })
                    .cloned();
                if let Some(cc) = callee_caps {
                    for cap in cc.rvs_iter() {
                        if !combined.rvs_contains(cap) {
                            combined.rvs_insert_M(cap);
                            changed = true;
                        }
                    }
                }
            }
            inferred.insert(caller.clone(), combined);
        }
    }
    inferred
}

fn rvs_format_capsmap(caps: &BTreeMap<String, CapabilitySet>) -> String {
    let mut lines: Vec<String> = caps
        .iter()
        .map(|(name, cs)| {
            let caps_str: String = cs.rvs_iter().map(|c| c.rvs_as_char()).collect();
            format!("{name}={caps_str}")
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
            if seed.rvs_lookup(callee).is_some() {
                continue;
            }
            if let Some(caps) = inferred.get(callee) {
                known.entry(callee.clone()).or_insert_with(|| caps.clone());
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

    fn rvs_snapshot(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    // ─── rvs_build_report ───────────────────────────────────────────────

    #[test]
    fn test_20260607_report_empty() {
        let entries = vec![];
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot("test_20260607_report_empty", &output);
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
        rvs_snapshot("test_20260607_report_pure_only", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 1);
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
        rvs_snapshot("test_20260607_report_mixed", &output);
        assert_eq!(report.total_fn_count, 3);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 2);
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
        rvs_snapshot("test_20260607_report_skips_test_and_dead_code", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.total_line_count, 10);
    }

    // ─── JSON parsing ───────────────────────────────────────────────────

    #[test]
    fn test_20260608_json_parse_empty() {
        let entries = rvs_parse_report_json("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_20260608_json_parse_single_pure() {
        let json =
            r#"[{"name":"rvs_add","caps":"","lines":5,"is_test":false,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].capabilities.rvs_is_empty());
        assert_eq!(entries[0].line_count, 5);
        assert!(!entries[0].is_test);
    }

    #[test]
    fn test_20260608_json_parse_with_caps() {
        let json = r#"[{"name":"rvs_write_BI","caps":"BI","lines":10,"is_test":false,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].capabilities.rvs_contains(Capability::B));
        assert!(entries[0].capabilities.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260608_json_parse_test_fn() {
        let json = r#"[{"name":"test_20260608_foo","caps":"P","lines":3,"is_test":true,"allows_dead_code":false}]"#;
        let entries = rvs_parse_report_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_test);
    }

    // ─── setup functions ────────────────────────────────────────────────

    #[test]
    fn test_20260607_setup_inject_clippy_empty() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        rvs_snapshot(
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

    #[test]
    fn test_20260607_setup_spawn_capsmap_empty() {
        let input = "HashMap::new=\n";
        let (result, count) = rvs_inject_spawn_capsmap_M(input);
        rvs_snapshot(
            "test_20260607_setup_spawn_capsmap_empty",
            &format!("count: {count}\n{result}"),
        );
        assert_eq!(count, setup::SPAWN_CAPSMAP_ENTRIES.len());
        assert!(result.contains("tokio::spawn=AS"));
    }

    #[test]
    fn test_20260607_setup_spawn_capsmap_idempotent() {
        let input = "HashMap::new=\n";
        let (first, c1) = rvs_inject_spawn_capsmap_M(input);
        let (second, c2) = rvs_inject_spawn_capsmap_M(&first);
        assert!(c1 > 0);
        assert_eq!(c2, 0);
        assert_eq!(first, second);
    }

    #[test]
    fn test_20260607_setup_spawn_capsmap_partial() {
        let input = "tokio::spawn=AS\nstd::thread::spawn=BS\n";
        let (result, count) = rvs_inject_spawn_capsmap_M(input);
        assert!(count > 0);
        assert!(count < setup::SPAWN_CAPSMAP_ENTRIES.len());
        assert!(result.contains("tokio::task::spawn=AS"));
    }

    // ─── rvs_infer_caps_M ────────────────────────────────────────────────

    /// Helper: build a default `ParsedFnBehavior` with all flags false and no calls.
    fn rvs_make_behavior() -> ParsedFnBehavior {
        ParsedFnBehavior {
            calls: BTreeSet::new(),
            has_async: false,
            has_unsafe_block: false,
            is_unsafe_fn: false,
            has_mut_param: false,
            has_panic: false,
            has_static_ref: false,
            has_static_mut_ref: false,
            has_thread_local_ref: false,
            is_trait_impl: false,
        }
    }

    #[test]
    fn test_20260609_infer_caps_empty_callgraph() {
        let callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        rvs_snapshot(
            "test_20260609_infer_caps_empty_callgraph",
            &format!("{result:?}"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_20260609_infer_caps_single_pure() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        callgraph.insert("my_crate::rvs_add".into(), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot("test_20260609_infer_caps_single_pure", &output);
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
        let mut behavior = rvs_make_behavior();
        behavior.has_panic = true;
        callgraph.insert("my_crate::rvs_divide_P".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot("test_20260609_infer_caps_single_panic", &output);
        let caps = result
            .get("my_crate::rvs_divide_P")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::P));
        assert_eq!(caps.rvs_len(), 1);
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
        rvs_snapshot("test_20260609_infer_caps_single_static_ref", &output);
        let caps = result
            .get("my_crate::rvs_get_env_S")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::S));
        assert_eq!(caps.rvs_len(), 1);
    }

    #[test]
    fn test_20260609_infer_caps_single_unsafe_block() {
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.has_unsafe_block = true;
        callgraph.insert("my_crate::rvs_ffi_call_U".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot("test_20260609_infer_caps_single_unsafe_block", &output);
        let caps = result
            .get("my_crate::rvs_ffi_call_U")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::U));
        assert_eq!(caps.rvs_len(), 1);
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
        rvs_snapshot(
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
        rvs_snapshot("test_20260609_infer_caps_propagation_chain", &output);

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
        rvs_snapshot("test_20260609_infer_caps_cycle_self_recursive", &output);

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
        rvs_snapshot("test_20260609_infer_caps_cycle_mutual_recursion", &output);

        // Mutual recursion with no caps: both stay empty
        assert!(result.get("my_crate::A").is_none_or(|c| c.rvs_is_empty()));
        assert!(result.get("my_crate::B").is_none_or(|c| c.rvs_is_empty()));
    }

    #[test]
    fn test_20260609_infer_caps_seed_override() {
        // Function is in seed with BI, but behavioral flags say has_panic=true.
        // Seed should win — the inferred result should only have BI, not P.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.has_panic = true;
        callgraph.insert("my_crate::rvs_read_BI".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("my_crate::rvs_read_BI=BI").unwrap();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot("test_20260609_infer_caps_seed_override", &output);

        let caps = result
            .get("my_crate::rvs_read_BI")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(
            !caps.rvs_contains(Capability::P),
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
        rvs_snapshot("test_20260609_infer_caps_rvs_suffix_from_name", &output);

        let caps = result
            .get("my_crate::rvs_write_db_ABM")
            .expect("should have entry");
        // A from has_async, M from has_mut_param
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::M));
        assert_eq!(caps.rvs_len(), 2);
    }

    // ─── rvs_format_capsmap ────────────────────────────────────────────

    #[test]
    fn test_20260609_format_capsmap_empty() {
        let map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let output = rvs_format_capsmap(&map);
        rvs_snapshot("test_20260609_format_capsmap_empty", &output);
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
        rvs_snapshot("test_20260609_format_capsmap_single_entry", &output);
        assert_eq!(output, "std::fs::read=BI\n");
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
        rvs_snapshot("test_20260609_format_capsmap_multiple_sorted", &output);
        // BTreeMap is already sorted, so output should be alphabetical by key
        let lines: Vec<&str> = output.trim_end().lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("HashMap::new"));
        assert!(lines[1].starts_with("std::fs::read"));
        assert!(lines[2].starts_with("std::process::exit"));
    }

    // ─── rvs_parse_callgraph ────────────────────────────────────────────

    #[test]
    fn test_20260609_parse_callgraph_valid_json() {
        let json = r#"{
            "my_crate::rvs_add": {
                "calls": ["my_crate::rvs_helper"],
                "has_async": false,
                "has_unsafe_block": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_panic": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            },
            "my_crate::rvs_write_BI": {
                "calls": ["std::fs::write"],
                "has_async": false,
                "has_unsafe_block": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_panic": true,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#;
        let result = rvs_parse_callgraph(json).unwrap();
        let output = format!("{result:?}");
        rvs_snapshot("test_20260609_parse_callgraph_valid_json", &output);
        assert_eq!(result.len(), 2);

        let add_behavior = result
            .get("my_crate::rvs_add")
            .expect("should find rvs_add");
        assert!(add_behavior.calls.contains("my_crate::rvs_helper"));
        assert!(!add_behavior.has_panic);

        let write_behavior = result
            .get("my_crate::rvs_write_BI")
            .expect("should find rvs_write_BI");
        assert!(write_behavior.calls.contains("std::fs::write"));
        assert!(write_behavior.has_panic);
    }

    #[test]
    fn test_20260609_parse_callgraph_invalid_json() {
        let json = "this is not json at all";
        let result = rvs_parse_callgraph(json);
        rvs_snapshot(
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
        behavior.calls.insert("std::time::impl::now".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

        let (known, unknown) =
            rvs_collect_direct_external_deps(&callgraph, "my_crate", &seed, &inferred);

        assert!(known.is_empty());
        assert!(
            unknown.contains_key("std::time::impl::now"),
            "unknown callee must be reported as error"
        );
        assert_eq!(unknown.len(), 1);
        assert!(unknown["std::time::impl::now"].contains("my_crate::caller"));
    }

    #[test]
    fn test_20260611_inferred_callee_is_known() {
        // If a callee IS in the inferred map, it goes to known, not unknown.
        let mut callgraph: BTreeMap<String, ParsedFnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::fs::read_to_string".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        inferred.insert(
            "std::fs::read_to_string".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );

        let (known, unknown) =
            rvs_collect_direct_external_deps(&callgraph, "my_crate", &seed, &inferred);

        let caps = known
            .get("std::fs::read_to_string")
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

        let (known, unknown) =
            rvs_collect_direct_external_deps(&callgraph, "my_crate", &seed, &inferred);

        assert!(!known.contains_key("std::fs::write"));
        assert!(!unknown.contains_key("std::fs::write"));
    }
}
