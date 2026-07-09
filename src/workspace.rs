use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::artifacts::{self, FnGraph};
use crate::capsmap::{self, CapsMap};
use crate::symbols::CrateName;

/// Resolve the capsmap path for the lint pass.
///
/// Only the project `caps/` directory is used. There is no built-in caps
/// fallback, no target/* caps cache, and no `-m` override.
fn rvs_resolve_capsmap_BIMS(cmd: &mut Command, project_path: &Path) -> Result<(), String> {
    let project_caps = project_path.join("caps");
    if rvs_validate_optional_capsmap_dir_BIS(&project_caps)? {
        rvs_load_project_caps_BIS(project_path)?;
        cmd.env("RIVUS_CAPSMAP", project_caps);
    }
    Ok(())
}

fn rvs_require_capsmap_dir_BIS(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!(
            "capsmap path must be a directory: {}",
            path.display()
        ));
    }
    CapsMap::rvs_load_dir_BIS(path)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub(crate) fn rvs_validate_optional_capsmap_dir_BIS(path: &Path) -> Result<bool, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(format!(
            "capsmap path must be a directory: {}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match std::fs::symlink_metadata(path)
        {
            Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Ok(_) => Err(format!(
                "capsmap path must be a directory: {}",
                path.display()
            )),
            Err(symlink_error) => Err(format!(
                "cannot inspect capsmap path {}: {symlink_error}",
                path.display()
            )),
        },
        Err(e) => Err(format!(
            "cannot inspect capsmap path {}: {e}",
            path.display()
        )),
    }
}

fn rvs_absolute_path_BIS(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map_err(|e| format!("current dir invalid: {e}"))
            .map(|cwd| cwd.join(path))
    }
}

/// Configuration for running `cargo check` with the rivus lint pass.
#[derive(Debug)]
pub(crate) struct CargoCheckConfig<'a> {
    pub(crate) project_path: &'a Path,
    /// Use RUSTC_WRAPPER (wraps all crates) instead of RUSTC_WORKSPACE_WRAPPER (workspace only).
    pub(crate) wrap_all_crates: bool,
    /// Pass --tests to cargo check.
    pub(crate) with_tests: bool,
    /// Use -Zbuild-std with nightly toolchain.
    pub(crate) build_std: bool,
    /// Extra environment variables to set.
    pub(crate) extra_env: Vec<(&'a str, OsString)>,
    /// Extra cargo check arguments.
    pub(crate) extra_args: Vec<&'a str>,
    /// Output subdirectory name under target/ (e.g. "rivus-build").
    /// If None, uses default target/ directory.
    pub(crate) target_subdir: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CargoCheckError {
    Message(String),
    ExitCode(i32),
}

impl CargoCheckError {
    pub(crate) fn rvs_exit_code(&self) -> i32 {
        match self {
            Self::Message(_) => 1,
            Self::ExitCode(code) => *code,
        }
    }
}

impl fmt::Display for CargoCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::ExitCode(code) => write!(f, "cargo check failed (exit code {code})"),
        }
    }
}

/// Runs `cargo check` with the rivus lint pass configured according to `config`.
/// Returns `Ok(())` on success, `Err(message)` on failure.
///
/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
pub(crate) fn rvs_run_cargo_check_impl_BIMS(
    config: &CargoCheckConfig,
) -> Result<(), CargoCheckError> {
    let mut cmd = rvs_prepare_cargo_check_command_BIMS(config)?;
    let exit_status = cmd
        .spawn()
        .map_err(|e| CargoCheckError::Message(format!("could not run cargo: {e}")))?
        .wait()
        .map_err(|e| CargoCheckError::Message(format!("failed to wait for cargo: {e}")))?;
    if !exit_status.success() {
        return Err(CargoCheckError::ExitCode(exit_status.code().unwrap_or(1)));
    }
    Ok(())
}

fn rvs_prepare_cargo_check_command_BIMS(
    config: &CargoCheckConfig,
) -> Result<Command, CargoCheckError> {
    let self_path = rvs_current_wrapper_exe_BIS()
        .map_err(|e| CargoCheckError::Message(format!("current executable path invalid: {e}")))?;
    let cargo = rvs_cargo_command_from_env_BS();
    let mut cmd = Command::new(&cargo);
    let project_path =
        rvs_absolute_path_BIS(config.project_path).map_err(CargoCheckError::Message)?;

    for key in [
        "RIVUS_CALLGRAPH",
        "RIVUS_CALLGRAPH_DIR",
        "RIVUS_CAPSMAP",
        "RIVUS_OFFLINE_CAPS",
        "RIVUS_ENABLED",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        cmd.env_remove(key);
    }

    if config.build_std {
        cmd.env("RUSTUP_TOOLCHAIN", "nightly");
    }
    cmd.current_dir(&project_path);

    let wrapper_env = if config.wrap_all_crates {
        "RUSTC_WRAPPER"
    } else {
        "RUSTC_WORKSPACE_WRAPPER"
    };
    for (key, val) in &config.extra_env {
        if *key == "RIVUS_CALLGRAPH" && val != "1" {
            return Err(CargoCheckError::Message(format!(
                "driver-controlled env {key} must be set to 1 when provided"
            )));
        }
        cmd.env(key, val);
    }

    for key in [
        "RIVUS_ENABLED",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        cmd.env_remove(key);
    }

    cmd.env(wrapper_env, &self_path).env("RIVUS_ENABLED", "1");

    let has_callgraph_env = config
        .extra_env
        .iter()
        .any(|(key, value)| *key == "RIVUS_CALLGRAPH" && value == "1");
    let has_capsmap_env = config
        .extra_env
        .iter()
        .any(|(key, _)| *key == "RIVUS_CAPSMAP");
    if has_capsmap_env {
        for (_, value) in config
            .extra_env
            .iter()
            .filter(|(key, _)| *key == "RIVUS_CAPSMAP")
        {
            let path = Path::new(value);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_path.join(path)
            };
            rvs_require_capsmap_dir_BIS(&resolved).map_err(CargoCheckError::Message)?;
            cmd.env("RIVUS_CAPSMAP", resolved);
        }
    }
    if !has_callgraph_env && !has_capsmap_env {
        rvs_resolve_capsmap_BIMS(&mut cmd, &project_path).map_err(CargoCheckError::Message)?;
    }

    cmd.arg("check");
    if config.with_tests {
        cmd.arg("--tests");
    }
    if config.build_std {
        cmd.arg("-Zbuild-std=std,core,alloc");
        cmd.arg("--target")
            .arg(rvs_host_triple_BIMS().map_err(CargoCheckError::Message)?);
    }
    if let Some(subdir) = config.target_subdir {
        let target_dir = project_path.join("target").join(subdir);
        cmd.arg("--target-dir").arg(&target_dir);
    }
    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    Ok(cmd)
}

fn rvs_cargo_command_from_env_BS() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn rvs_current_wrapper_exe_BIS() -> Result<PathBuf, std::io::Error> {
    let current = env::current_exe()?;
    if let Some(parent) = current.parent()
        && parent.file_name().is_some_and(|name| name == "deps")
        && let Some(debug_dir) = parent.parent()
    {
        let candidate = debug_dir.join("cargo-rivus");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Ok(current)
}

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
pub(crate) fn rvs_run_cargo_check_BIMS(extra_args: &[String]) -> Result<(), i32> {
    if let Err(e) = rvs_reject_forwarded_check_args(extra_args) {
        eprintln!("{e}");
        return Err(2);
    }
    let project_path = Path::new(".");
    let extra_args_ref: Vec<&str> = extra_args.iter().map(|arg| arg.as_str()).collect();
    match rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
        project_path,
        wrap_all_crates: false,
        with_tests: true,
        build_std: false,
        extra_env: vec![("RIVUS_OFFLINE_CAPS", "1".into())],
        extra_args: extra_args_ref.clone(),
        target_subdir: None,
    }) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{e}");
            return Err(e.rvs_exit_code());
        }
    }

    let mut callgraph = match rvs_collect_callgraph_with_args_BIMS(
        project_path,
        false,
        true,
        vec![],
        extra_args_ref,
    ) {
        Ok(callgraph) => callgraph,
        Err(e) => {
            eprintln!("offline caps check unavailable: {e}");
            return Err(1);
        }
    };
    let caps = match rvs_load_project_caps_BIS(project_path) {
        Ok(caps) => caps,
        Err(e) => {
            eprintln!("offline caps check cannot load caps/: {e}");
            return Err(1);
        }
    };
    let local_crate_names = match rvs_load_local_crate_prefixes_BIS(project_path) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("offline caps check cannot detect local crates: {e}");
            return Err(1);
        }
    };
    let report =
        crate::offline_caps::rvs_check_offline_caps_M(&mut callgraph, &caps, &local_crate_names);
    if !report.rvs_is_empty() {
        print!("{report}");
    }
    if report.rvs_has_errors() {
        Err(1)
    } else {
        Ok(())
    }
}

fn rvs_reject_forwarded_check_args(extra_args: &[String]) -> Result<(), String> {
    let mut index = 0usize;
    while index < extra_args.len() {
        let Some(arg) = extra_args.get(index) else {
            break;
        };
        if arg == "--manifest-path" || arg.starts_with("--manifest-path=") {
            return Err("cargo rivus check does not support forwarded --manifest-path; run from the target project directory or use commands with an explicit path".into());
        }
        if arg == "--config" {
            let Some(value) = extra_args.get(index + 1) else {
                return Err("cargo rivus check does not support empty forwarded --config".into());
            };
            rvs_reject_dangerous_forwarded_config(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            rvs_reject_dangerous_forwarded_config(value)?;
        }
        index += 1;
    }
    Ok(())
}

fn rvs_reject_dangerous_forwarded_config(value: &str) -> Result<(), String> {
    let dangerous_keys = [
        "build.rustc",
        "build.rustc-wrapper",
        "build.rustc-workspace-wrapper",
        "env.RIVUS_ENABLED",
        "env.RIVUS_CAPSMAP",
        "env.RIVUS_CALLGRAPH",
        "env.RIVUS_CALLGRAPH_DIR",
        "env.RUSTC",
        "env.RUSTC_WRAPPER",
        "env.RUSTC_WORKSPACE_WRAPPER",
    ];
    if !value.contains('=') {
        return Err("cargo rivus check does not support forwarded --config paths; pass cargo config through the environment instead".into());
    }
    let doc = value
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("cannot parse forwarded --config value: {e}"))?;
    let mut key_paths = Vec::new();
    for (key, item) in doc.iter() {
        rvs_collect_config_key_paths_M(key, item, &mut key_paths);
    }
    for key in key_paths {
        if dangerous_keys
            .iter()
            .any(|dangerous| key == *dangerous || key.starts_with(&format!("{dangerous}.")))
        {
            return Err(format!(
                "cargo rivus check does not support forwarded --config for driver-controlled key '{key}'"
            ));
        }
    }
    Ok(())
}

fn rvs_collect_config_key_paths_M(prefix: &str, item: &toml_edit::Item, out: &mut Vec<String>) {
    out.push(prefix.to_string());
    if let Some(table) = item.as_table_like() {
        for (key, child) in table.iter() {
            let child_prefix = format!("{prefix}.{key}");
            rvs_collect_config_key_paths_M(&child_prefix, child, out);
        }
    }
}

/// Unified callgraph collector.
///
/// Cleans the callgraph output directory and build directory, runs
/// `cargo check` with the callgraph collection environment, and returns
/// the merged callgraph.
///
/// - `build_std=false` -> wraps all crates (RUSTC_WRAPPER), uses `target/rivus-build`
/// - `build_std=true`  -> wraps all crates + `-Zbuild-std`, uses `target/rivus-build-std`
///
/// `extra_env` is merged into the cargo environment, useful for passing
/// `RIVUS_CAPSMAP` to the lint subprocess.
///
/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_collect_callgraph_BIMS(
    path: &Path,
    build_std: bool,
    with_tests: bool,
    extra_env: Vec<(&str, OsString)>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_BIMS(path, build_std, with_tests, extra_env, vec![])
}

pub(crate) fn rvs_collect_callgraph_with_args_BIMS(
    path: &Path,
    build_std: bool,
    with_tests: bool,
    extra_env: Vec<(&str, OsString)>,
    extra_args: Vec<&str>,
) -> Result<FnGraph, String> {
    let suffix = if build_std { "-std" } else { "" };
    let cg_subdir = format!("rivus-callgraph{suffix}");
    let build_subdir = format!("rivus-build{suffix}");

    let cg_dir = path.join("target").join(&cg_subdir);
    let abs_cg_dir = std::env::current_dir()
        .map_err(|e| format!("current dir invalid: {e}"))?
        .join(&cg_dir);

    rvs_clean_dir_BIS(&cg_dir)?;
    rvs_clean_dir_BIS(&path.join("target").join(&build_subdir))?;

    let env_vars = rvs_callgraph_collection_env(extra_env, abs_cg_dir.into_os_string());

    rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates: true,
        with_tests,
        build_std,
        extra_env: env_vars,
        extra_args,
        target_subdir: Some(&build_subdir),
    })
    .map_err(|e| e.to_string())?;

    rvs_merge_callgraph_dir_BIS(&cg_dir)
}

fn rvs_callgraph_collection_env(
    extra_env: Vec<(&str, OsString)>,
    abs_cg_dir: OsString,
) -> Vec<(&str, OsString)> {
    let mut env_vars = extra_env;
    env_vars.push(("RIVUS_CALLGRAPH", "1".into()));
    env_vars.push(("RIVUS_CALLGRAPH_DIR", abs_cg_dir));
    env_vars
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedCallgraphMode {
    ProjectRequired,
    StdOnlyAllowed,
}

fn rvs_load_or_collect_callgraph_BIMS(
    path: &Path,
    mode: CachedCallgraphMode,
) -> Result<FnGraph, String> {
    rvs_load_or_collect_callgraph_with_collector_BIMS(path, mode, |project_path| {
        rvs_collect_callgraph_BIMS(project_path, false, true, vec![])
    })
}

fn rvs_load_or_collect_callgraph_with_collector_BIMS(
    path: &Path,
    mode: CachedCallgraphMode,
    collect_fresh_BIMS: impl FnOnce(&Path) -> Result<FnGraph, String>,
) -> Result<FnGraph, String> {
    let cg_dir = path.join("target").join("rivus-callgraph");
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");

    if mode == CachedCallgraphMode::StdOnlyAllowed {
        if rvs_validate_optional_dir_BIS(&cg_std_dir, "std callgraph cache")? {
            let cg = rvs_merge_callgraph_dir_BIS(&cg_std_dir)
                .map_err(|e| format!("{e}; run cargo rivus infer-std first"))?;
            let mut std_only = FnGraph::rvs_new();
            rvs_merge_std_like_callgraph_M(&mut std_only, cg);
            if !std_only.rvs_is_empty() {
                return Ok(std_only);
            }
        }
        return Err("std callgraph cache not found; run cargo rivus infer-std first".into());
    }

    let local_prefixes = match rvs_detect_local_crate_prefixes_BIS(path) {
        Ok(prefixes) => prefixes,
        Err(e) => {
            eprintln!("warning: cannot detect local crate prefixes for std cache filtering: {e}");
            BTreeSet::new()
        }
    };
    if cg_dir.is_dir() {
        match rvs_merge_callgraph_dir_BIS(&cg_dir) {
            Ok(project) if !project.rvs_is_empty() => {
                let mut merged = project;
                if rvs_warn_optional_dir_BIS(&cg_std_dir, "std callgraph cache") {
                    match rvs_merge_callgraph_dir_BIS(&cg_std_dir) {
                        Ok(cg) => rvs_merge_std_like_callgraph_with_local_prefixes_M(
                            &mut merged,
                            cg,
                            &local_prefixes,
                        ),
                        Err(e) => eprintln!("warning: ignoring stale std callgraph cache: {e}"),
                    }
                }
                return Ok(merged);
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("warning: ignoring stale project callgraph cache: {e}");
            }
        }
    }
    eprintln!("(no cached project callgraph found, collecting fresh...)");
    let mut collected = collect_fresh_BIMS(path)?;
    if rvs_warn_optional_dir_BIS(&cg_std_dir, "std callgraph cache") {
        match rvs_merge_callgraph_dir_BIS(&cg_std_dir) {
            Ok(cg) => rvs_merge_std_like_callgraph_with_local_prefixes_M(
                &mut collected,
                cg,
                &local_prefixes,
            ),
            Err(e) => eprintln!("warning: ignoring stale std callgraph cache: {e}"),
        }
    }
    Ok(collected)
}

fn rvs_merge_std_like_callgraph_M(target: &mut FnGraph, source: FnGraph) {
    rvs_merge_std_like_callgraph_with_local_prefixes_M(target, source, &BTreeSet::new());
}

fn rvs_merge_std_like_callgraph_with_local_prefixes_M(
    target: &mut FnGraph,
    source: FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let mut filtered = FnGraph::rvs_new();
    for (path, node) in source.nodes {
        if rvs_is_std_like_def_path(path.rvs_as_str())
            && !rvs_function_matches_local_prefix(path.rvs_as_str(), local_crate_names)
        {
            filtered.rvs_insert_M(path, node);
        }
    }
    target.rvs_merge_from_M(filtered);
}

pub(crate) fn rvs_load_project_caps_BIS(path: &Path) -> Result<capsmap::CapsMap, String> {
    let caps_dir = path.join("caps");
    if !rvs_validate_optional_capsmap_dir_BIS(&caps_dir)? {
        return Ok(CapsMap::rvs_new());
    }
    CapsMap::rvs_load_dir_BIS(&caps_dir).map_err(|e| format!("caps/: {e}"))
}

pub(crate) fn rvs_load_callgraph_and_caps_for_function_BIMS(
    path: &Path,
    function: &str,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let mode = rvs_cached_callgraph_mode_for_function_BIS(path, function)?;
    let callgraph = if mode == CachedCallgraphMode::StdOnlyAllowed {
        rvs_load_or_collect_callgraph_BIMS(path, mode)?
    } else {
        rvs_collect_project_callgraph_with_optional_std_cache_BIMS(path, true)?
    };
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_cached_callgraph_mode_for_function_BIS(
    path: &Path,
    function: &str,
) -> Result<CachedCallgraphMode, String> {
    if rvs_is_std_like_def_path(function) && !rvs_is_local_function_query_BIS(path, function)? {
        Ok(CachedCallgraphMode::StdOnlyAllowed)
    } else {
        Ok(CachedCallgraphMode::ProjectRequired)
    }
}

fn rvs_is_local_function_query_BIS(path: &Path, function: &str) -> Result<bool, String> {
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read '{}': {e}", cargo_toml.display()))?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("invalid TOML in '{}': {e}", cargo_toml.display()))?;
    if doc.get("package").is_none() {
        return Ok(false);
    }
    rvs_load_local_crate_prefixes_BIS(path)
        .map(|names| rvs_function_matches_local_prefix(function, &names))
}

pub(crate) fn rvs_function_matches_local_prefix(
    function: &str,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    local_crate_names
        .iter()
        .any(|name| function.starts_with(name.rvs_prefix().rvs_as_str()))
}

fn rvs_is_std_like_def_path(function: &str) -> bool {
    function.starts_with("std::")
        || function.starts_with("core::")
        || function.starts_with("alloc::")
        || function.starts_with("compiler_builtins::")
}

pub(crate) fn rvs_collect_callgraph_and_caps_BIMS(
    path: &Path,
    with_tests: bool,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let callgraph = rvs_collect_project_callgraph_with_optional_std_cache_BIMS(path, with_tests)?;
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_collect_project_callgraph_with_optional_std_cache_BIMS(
    path: &Path,
    with_tests: bool,
) -> Result<FnGraph, String> {
    let mut callgraph = rvs_collect_callgraph_BIMS(path, false, with_tests, vec![])?;
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");
    if rvs_warn_optional_dir_BIS(&cg_std_dir, "std callgraph cache") {
        let local_prefixes = match rvs_detect_local_crate_prefixes_BIS(path) {
            Ok(prefixes) => prefixes,
            Err(e) => {
                eprintln!(
                    "warning: cannot detect local crate prefixes for std cache filtering: {e}"
                );
                BTreeSet::new()
            }
        };
        match rvs_merge_callgraph_dir_BIS(&cg_std_dir) {
            Ok(std_graph) => rvs_merge_std_like_callgraph_with_local_prefixes_M(
                &mut callgraph,
                std_graph,
                &local_prefixes,
            ),
            Err(e) => eprintln!("warning: ignoring stale std callgraph cache: {e}"),
        }
    }
    Ok(callgraph)
}

fn rvs_validate_optional_dir_BIS(path: &Path, label: &str) -> Result<bool, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(format!("{label} must be a directory: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Ok(_) => Err(format!("{label} must be a directory: {}", path.display())),
                Err(symlink_error) => Err(format!(
                    "cannot inspect {label} {}: {symlink_error}",
                    path.display()
                )),
            }
        }
        Err(e) => Err(format!("cannot inspect {label} {}: {e}", path.display())),
    }
}

fn rvs_warn_optional_dir_BIS(path: &Path, label: &str) -> bool {
    match rvs_validate_optional_dir_BIS(path, label) {
        Ok(exists) => exists,
        Err(e) => {
            eprintln!("warning: ignoring stale {label}: {e}");
            false
        }
    }
}

pub(crate) fn rvs_write_capsmap_result_BIS(
    result: &str,
    output: &Path,
    label: &str,
) -> Result<(), String> {
    rvs_preflight_capsmap_file_BIS(output, label)?;
    rvs_write_capsmap_file_BIS(output, result, label)?;
    println!("Written {label} to {}", output.display());
    Ok(())
}

#[cfg(test)]
fn rvs_capsmap_output_paths_overlap_BIS(left: &Path, right: &Path) -> Result<bool, String> {
    debug_assert!(!left.as_os_str().is_empty(), "left path must not be empty");
    debug_assert!(
        !right.as_os_str().is_empty(),
        "right path must not be empty"
    );
    let left = rvs_normalize_existing_ancestor_path_BIS(left)?;
    let right = rvs_normalize_existing_ancestor_path_BIS(right)?;
    Ok(left == right || left.starts_with(&right) || right.starts_with(&left))
}

pub(crate) fn rvs_preflight_capsmap_file_BIS(path: &Path, label: &str) -> Result<(), String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let separator = std::path::MAIN_SEPARATOR as u8;
    let mut end = bytes.len();
    while end > 0 && bytes.get(end - 1).is_some_and(|byte| *byte == separator) {
        end -= 1;
    }
    let trimmed = bytes.get(..end).unwrap_or(bytes);
    let has_trailing_separator = end < bytes.len();
    let ends_in_dot_component = trimmed == b"."
        || trimmed == b".."
        || trimmed.ends_with(&[separator, b'.'])
        || trimmed.ends_with(&[separator, b'.', b'.']);
    if has_trailing_separator
        || ends_in_dot_component
        || !matches!(path.components().next_back(), Some(std::path::Component::Normal(name)) if !name.is_empty())
    {
        return Err(format!(
            "{label} output path must name a file: {}",
            path.display()
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{label} output path must not contain '..': {}",
            path.display()
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "{label} output must be a regular file, not a symlink: {}",
            path.display()
        )),
        Ok(metadata) if metadata.is_dir() => {
            Err(format!("{label} output must be a file: {}", path.display()))
        }
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "{label} output must be a regular file: {}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "cannot inspect {label} output {}: {e}",
            path.display()
        )),
    }?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        match std::fs::metadata(parent) {
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(format!(
                "{label} output parent must be a directory: {}",
                parent.display()
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::symlink_metadata(parent) {
                    Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                        Ok(())
                    }
                    Ok(_) => Err(format!(
                        "{label} output parent must be a directory: {}",
                        parent.display()
                    )),
                    Err(symlink_error) => Err(format!(
                        "cannot inspect {label} output parent {}: {symlink_error}",
                        parent.display()
                    )),
                }
            }
            Err(e) => Err(format!(
                "cannot inspect {label} output parent {}: {e}",
                parent.display()
            )),
        }?;
    }
    Ok(())
}

pub(crate) fn rvs_write_capsmap_file_BIS(
    path: &Path,
    result: &str,
    label: &str,
) -> Result<(), String> {
    rvs_preflight_capsmap_file_BIS(path, label)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create parent for {}: {e}", path.display()))?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "capsmap".into());
    let mut tmp_path: Option<PathBuf> = None;
    let write_result = (|| -> Result<(), String> {
        let mut file = None;
        for attempt in 0..100usize {
            debug_assert!(attempt < 100, "temp filename retry bound");
            let candidate = path.with_file_name(format!(
                ".{file_name}.{}.{}.tmp",
                std::process::id(),
                attempt
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(opened) => {
                    tmp_path = Some(candidate);
                    file = Some(opened);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("cannot create {}: {e}", candidate.display())),
            }
        }
        let mut file = file.ok_or_else(|| {
            format!(
                "cannot create temp capsmap file for {}: too many collisions",
                path.display()
            )
        })?;
        let tmp_path = tmp_path
            .as_ref()
            .expect("never: temp path set when temp file was opened");
        file.write_all(result.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| format!("cannot sync {}: {e}", tmp_path.display()))?;
        drop(file);
        std::fs::rename(tmp_path, path).map_err(|e| {
            format!(
                "cannot rename {} to {}: {e}",
                tmp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    })();
    if write_result.is_err()
        && let Some(tmp_path) = &tmp_path
        && let Err(_cleanup_error) = std::fs::remove_file(tmp_path)
    {}
    write_result
}

#[cfg(test)]
fn rvs_normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match normalized.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(std::path::Component::ParentDir) | None if !normalized.has_root() => {
                    normalized.push(component.as_os_str());
                }
                Some(
                    std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
                    | std::path::Component::CurDir,
                )
                | Some(std::path::Component::ParentDir)
                | None => {}
            },
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
fn rvs_normalize_existing_ancestor_path_BIS(path: &Path) -> Result<PathBuf, String> {
    debug_assert!(
        !path.as_os_str().is_empty(),
        "path identity input must not be empty"
    );
    let components: Vec<_> = path
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    for split in (0..=components.len()).rev() {
        let prefix = components
            .get(..split)
            .unwrap_or(&[])
            .iter()
            .collect::<PathBuf>();
        let canonical_prefix = if prefix.as_os_str().is_empty() {
            std::env::current_dir().map_err(|e| format!("current dir invalid: {e}"))
        } else {
            prefix
                .canonicalize()
                .map_err(|e| format!("cannot canonicalize '{}': {e}", prefix.display()))
        };
        if let Ok(mut normalized) = canonical_prefix {
            for component in components.get(split..).unwrap_or(&[]) {
                normalized.push(component);
            }
            return Ok(rvs_normalize_path_lexically(&normalized));
        }
    }
    Ok(rvs_normalize_path_lexically(path))
}

#[cfg(test)]
pub(crate) fn rvs_collect_local_crate_prefixes(toml: &str) -> Result<BTreeSet<CrateName>, String> {
    rvs_collect_local_crate_prefixes_for_targets(toml, true)
}

fn rvs_collect_local_crate_prefixes_for_targets(
    toml: &str,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let doc: toml_edit::DocumentMut = toml.parse().map_err(|e| format!("invalid TOML: {e}"))?;

    let mut prefixes = BTreeSet::new();
    if let Some(package_name) = doc
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[package].name", package_name)?;
    }
    if let Some(lib_name) = doc
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(|name| name.as_str())
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[lib].name", lib_name)?;
    }
    if let Some(bins) = doc.get("bin").and_then(toml_edit::Item::as_array_of_tables) {
        for bin in bins {
            if let Some(name) = bin.get("name").and_then(|name| name.as_str()) {
                rvs_insert_manifest_crate_name_M(&mut prefixes, "[[bin]].name", name)?;
            }
        }
    }
    if include_test_example_bench {
        for table_name in ["test", "example", "bench"] {
            if let Some(targets) = doc
                .get(table_name)
                .and_then(toml_edit::Item::as_array_of_tables)
            {
                for target in targets {
                    if let Some(name) = target.get("name").and_then(|name| name.as_str()) {
                        rvs_insert_manifest_crate_name_M(
                            &mut prefixes,
                            &format!("[[{table_name}]].name"),
                            name,
                        )?;
                    }
                }
            }
        }
    }
    if prefixes.is_empty() {
        return Err(
            "Cargo.toml: missing local crate target ([package].name, [lib].name, [[bin]].name, [[test]].name, [[example]].name, or [[bench]].name)"
                .into(),
        );
    }
    Ok(prefixes)
}

fn rvs_insert_manifest_crate_name_M(
    prefixes: &mut BTreeSet<CrateName>,
    label: &str,
    name: &str,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(format!("{label} must not contain whitespace"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(format!("{label} must be a crate name, not a path"));
    }
    prefixes.insert(CrateName::rvs_from_manifest_name(name));
    Ok(())
}

pub(crate) fn rvs_detect_local_crate_prefixes_BIS(
    path: &Path,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_detect_local_crate_prefixes_for_targets_BIS(path, true)
}

pub(crate) fn rvs_detect_local_crate_prefixes_for_cargo_check_BIS(
    path: &Path,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_detect_local_crate_prefixes_for_targets_BIS(path, include_test_example_bench)
}

fn rvs_detect_local_crate_prefixes_for_targets_BIS(
    path: &Path,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let mut prefixes =
        rvs_collect_local_crate_prefixes_for_targets(&content, include_test_example_bench)
            .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    rvs_collect_auto_target_prefixes_for_targets_BIMS(
        path,
        &mut prefixes,
        &include_test_example_bench,
    )?;
    Ok(prefixes)
}

#[cfg(test)]
fn rvs_collect_auto_target_prefixes_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    rvs_collect_auto_target_prefixes_for_targets_BIMS(path, prefixes, &true)
}

fn rvs_collect_auto_target_prefixes_for_targets_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
    include_test_example_bench: &bool,
) -> Result<(), String> {
    let flags = rvs_collect_auto_target_flags_BIS(path)?;
    if *include_test_example_bench && flags.autotests {
        let tests_dir = path.join("tests");
        rvs_collect_rs_file_stems_BIMS(&tests_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&tests_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autoexamples {
        let examples_dir = path.join("examples");
        rvs_collect_rs_file_stems_BIMS(&examples_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&examples_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autobenches {
        let benches_dir = path.join("benches");
        rvs_collect_rs_file_stems_BIMS(&benches_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&benches_dir, prefixes)?;
    }
    if !flags.autobins {
        return Ok(());
    }
    let bin_dir = path.join("src/bin");
    rvs_collect_rs_file_stems_BIMS(&bin_dir, prefixes)?;
    rvs_collect_dir_target_names_BIMS(&bin_dir, prefixes)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AutoTargetFlags {
    autobins: bool,
    autotests: bool,
    autoexamples: bool,
    autobenches: bool,
}

fn rvs_collect_auto_target_flags_BIS(path: &Path) -> Result<AutoTargetFlags, String> {
    let mut flags = AutoTargetFlags {
        autobins: true,
        autotests: true,
        autoexamples: true,
        autobenches: true,
    };
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    let Some(package) = doc.get("package") else {
        return Ok(flags);
    };
    if let Some(value) = package.get("autobins").and_then(toml_edit::Item::as_bool) {
        flags.autobins = value;
    }
    if let Some(value) = package.get("autotests").and_then(toml_edit::Item::as_bool) {
        flags.autotests = value;
    }
    if let Some(value) = package
        .get("autoexamples")
        .and_then(toml_edit::Item::as_bool)
    {
        flags.autoexamples = value;
    }
    if let Some(value) = package
        .get("autobenches")
        .and_then(toml_edit::Item::as_bool)
    {
        flags.autobenches = value;
    }
    Ok(flags)
}

fn rvs_collect_rs_file_stems_BIMS(
    dir: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "cannot read auto target dir {}: {e}",
                dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .ok_or_else(|| format!("auto target file has no stem: {}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("auto target file stem is not UTF-8: {}", path.display()))?;
            rvs_insert_manifest_crate_name_M(prefixes, "auto target file stem", stem)?;
        }
    }
    Ok(())
}

fn rvs_collect_dir_target_names_BIMS(
    dir: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "cannot read auto target dir {}: {e}",
                dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.join("main.rs").is_file() {
            let entry_name = entry.file_name();
            let name = entry_name.to_str().ok_or_else(|| {
                format!(
                    "auto target directory name is not UTF-8: {}",
                    path.display()
                )
            })?;
            rvs_insert_manifest_crate_name_M(prefixes, "auto target directory name", name)?;
        }
    }
    Ok(())
}

pub(crate) fn rvs_load_local_crate_prefixes_BIS(
    path: &Path,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_ensure_cargo_project_BIS(path)?;
    rvs_detect_local_crate_prefixes_BIS(path)
}

pub(crate) fn rvs_clean_dir_BIS(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path)
            .map_err(|e| format!("cannot remove {}: {e}", path.display()))?,
        Ok(_) => std::fs::remove_file(path)
            .map_err(|e| format!("cannot remove {}: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot inspect {}: {e}", path.display())),
    }
    Ok(())
}

fn rvs_merge_callgraph_dir_BIS(cg_dir: &Path) -> Result<FnGraph, String> {
    let mut merged = FnGraph::rvs_new();
    let mut json_paths = Vec::new();
    let cg_entries =
        std::fs::read_dir(cg_dir).map_err(|e| format!("cannot read {}: {e}", cg_dir.display()))?;
    for entry in cg_entries {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", cg_dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", entry.path().display()))?;
        let path = entry.path();
        if file_type.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            json_paths.push(path);
        }
    }
    json_paths.sort();
    for path in &json_paths {
        let json_str = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        rvs_reject_stale_callgraph_json(path, &json_str)?;
        let partial = artifacts::rvs_parse_callgraph_json_S(&json_str)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        merged.rvs_merge_from_M(partial);
    }
    if json_paths.is_empty() {
        return Err(format!(
            "no callgraph JSON artifacts found in {}",
            cg_dir.display()
        ));
    }
    if merged.rvs_is_empty() {
        return Err(format!(
            "callgraph JSON artifacts in {} contained no nodes",
            cg_dir.display()
        ));
    }
    Ok(merged)
}

fn rvs_reject_stale_callgraph_json(path: &Path, json: &str) -> Result<(), String> {
    let entries: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, serde_json::Value>,
    > = serde_json::from_str(json)
        .map_err(|e| format!("invalid callgraph JSON in {}: {e}", path.display()))?;
    for (def_path, node) in &entries {
        if !node.contains_key("has_body") {
            return Err(format!(
                "stale callgraph JSON lacks has_body for {def_path}: {}; delete the stale cache or run cargo rivus infer-std for std cache",
                path.display()
            ));
        }
    }
    Ok(())
}

fn rvs_host_triple_BIMS() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("cannot run rustc -vV to determine host target: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc -vV failed while determining host target: {}",
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            return Ok(host.trim().to_string());
        }
    }
    Err("rustc -vV output did not contain a host target".into())
}

/// Validate that `path` is a directory, returning an error message if not.
pub(crate) fn rvs_ensure_project_dir_BS(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }
    Ok(())
}

pub(crate) fn rvs_ensure_cargo_project_BIS(path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Err(format!("'{}' is not a Cargo project", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    fn rvs_make_workspace_temp_dir_BIS(tag: &str) -> PathBuf {
        rvs_make_temp_dir_BIS(&format!("workspace-{tag}"))
    }

    #[test]
    fn test_20260630_collect_local_crate_prefixes_bin_name() {
        let input = "[package]\nname = \"rivus-linter\"\n\n[[bin]]\nname = \"cargo-rivus\"\npath = \"src/main.rs\"\n";
        let prefixes = rvs_collect_local_crate_prefixes(input).expect("prefixes should parse");
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260630_collect_local_crate_prefixes_bin_name",
            &output,
        );
        assert!(prefixes.contains("rivus_linter"));
        assert!(prefixes.contains("cargo_rivus"));
    }

    #[test]
    fn test_20260702_collect_local_crate_prefixes_rejects_workspace_root() {
        let input = "[workspace]\nmembers = []\nresolver = \"2\"\n";
        let result = rvs_collect_local_crate_prefixes(input);
        let output = format!("{result:?}");
        rvs_snapshot_BIS(
            "test_20260702_collect_local_crate_prefixes_rejects_workspace_root",
            &output,
        );
        assert!(result.is_err());
        assert!(output.contains("missing local crate target"));
    }

    #[test]
    fn test_20260707_collect_local_crate_prefixes_rejects_empty_target_names() {
        let package = rvs_collect_local_crate_prefixes("[package]\nname = \"\"\n");
        let bin = rvs_collect_local_crate_prefixes("[[bin]]\nname = \"\"\n");
        let mut prefixes = BTreeSet::new();
        let helper = rvs_insert_manifest_crate_name_M(&mut prefixes, "demo", "");
        let output = format!(
            "package={package:?}\nbin={bin:?}\nhelper={helper:?}\nlen={}\n",
            prefixes.len()
        );
        rvs_snapshot_BIS(
            "test_20260707_collect_local_crate_prefixes_rejects_empty_target_names",
            &output,
        );

        assert!(package.is_err());
        assert!(bin.is_err());
        assert!(helper.is_err());
        assert!(prefixes.is_empty());
    }

    #[test]
    fn test_20260707_collect_local_crate_prefixes_rejects_whitespace_target_names() {
        let package = rvs_collect_local_crate_prefixes("[package]\nname = \"bad name\"\n");
        let lib = rvs_collect_local_crate_prefixes("[lib]\nname = \" bad\"\n");
        let mut prefixes = BTreeSet::new();
        let helper = rvs_insert_manifest_crate_name_M(&mut prefixes, "demo", "bad\tname");
        let output = format!(
            "package={package:?}\nlib={lib:?}\nhelper={helper:?}\nlen={}\n",
            prefixes.len()
        );
        rvs_snapshot_BIS(
            "test_20260707_collect_local_crate_prefixes_rejects_whitespace_target_names",
            &output,
        );

        assert!(package.is_err());
        assert!(lib.is_err());
        assert!(helper.is_err());
        assert!(prefixes.is_empty());
    }

    #[test]
    fn test_20260707_collect_local_crate_prefixes_rejects_pathy_target_names() {
        let package = rvs_collect_local_crate_prefixes("[package]\nname = \"bad/name\"\n");
        let lib = rvs_collect_local_crate_prefixes("[lib]\nname = \"bad\\\\name\"\n");
        let mut prefixes = BTreeSet::new();
        let helper = rvs_insert_manifest_crate_name_M(&mut prefixes, "demo", "bad\0name");
        let output = format!(
            "package={package:?}\nlib={lib:?}\nhelper={helper:?}\nlen={}\n",
            prefixes.len()
        );
        rvs_snapshot_BIS(
            "test_20260707_collect_local_crate_prefixes_rejects_pathy_target_names",
            &output,
        );

        assert!(package.is_err());
        assert!(lib.is_err());
        assert!(helper.is_err());
        assert!(prefixes.is_empty());
    }

    #[test]
    fn test_20260705_collect_local_crate_prefixes_test_example_bench() {
        let input = r#"
[[test]]
name = "integration-test"

[[example]]
name = "demo-example"

[[bench]]
name = "throughput-bench"
"#;
        let prefixes = rvs_collect_local_crate_prefixes(input).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_collect_local_crate_prefixes_test_example_bench",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("integration_test")));
        assert!(prefixes.contains(&CrateName::from("demo_example")));
        assert!(prefixes.contains(&CrateName::from("throughput_bench")));
    }

    #[test]
    fn test_20260706_collect_prefixes_for_build_targets() {
        let input = r#"
[package]
name = "core-demo"

[[bin]]
name = "tool-bin"

[[test]]
name = "integration-test"

[[example]]
name = "demo-example"

[[bench]]
name = "throughput-bench"
"#;
        let prefixes = rvs_collect_local_crate_prefixes_for_targets(input, false).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS("test_20260706_collect_prefixes_for_build_targets", &output);

        assert!(prefixes.contains(&CrateName::from("core_demo")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(!prefixes.contains(&CrateName::from("integration_test")));
        assert!(!prefixes.contains(&CrateName::from("demo_example")));
        assert!(!prefixes.contains(&CrateName::from("throughput_bench")));
    }

    #[test]
    fn test_20260705_detect_local_crate_prefixes_auto_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-prefixes");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"auto-target-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo-example.rs"),
            ("benches", "fast-bench.rs"),
            ("src/bin", "tool-bin.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }
        std::fs::create_dir_all(dir.join("src/bin/server")).unwrap();
        std::fs::write(dir.join("src/bin/server/main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(dir.join("examples/nested-example")).unwrap();
        std::fs::write(
            dir.join("examples/nested-example/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests/support")).unwrap();

        let mut direct_prefixes = BTreeSet::new();
        rvs_collect_rs_file_stems_BIMS(&dir.join("tests"), &mut direct_prefixes).unwrap();
        rvs_collect_dir_target_names_BIMS(&dir.join("examples"), &mut direct_prefixes).unwrap();
        rvs_collect_auto_target_prefixes_BIMS(&dir, &mut direct_prefixes).unwrap();
        let prefixes = rvs_detect_local_crate_prefixes_BIS(&dir).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_detect_local_crate_prefixes_auto_targets",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("ui_tests")));
        assert!(prefixes.contains(&CrateName::from("demo_example")));
        assert!(prefixes.contains(&CrateName::from("fast_bench")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(prefixes.contains(&CrateName::from("server")));
        assert!(prefixes.contains(&CrateName::from("nested_example")));
        assert!(!prefixes.contains(&CrateName::from("support")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260705_detect_local_crate_prefixes_respects_auto_flags() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-prefix-flags");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"auto-flag-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautobins = false\nautotests = false\nautoexamples = false\nautobenches = false\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo.rs"),
            ("benches", "bench.rs"),
            ("src/bin", "tool.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }

        let prefixes = rvs_detect_local_crate_prefixes_BIS(&dir).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_detect_local_crate_prefixes_respects_auto_flags",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("auto_flag_demo")));
        assert!(!prefixes.contains(&CrateName::from("ui_tests")));
        assert!(!prefixes.contains(&CrateName::from("demo")));
        assert!(!prefixes.contains(&CrateName::from("bench")));
        assert!(!prefixes.contains(&CrateName::from("tool")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_reports_invalid_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"demo\"\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_reports_invalid_cargo_toml",
            &format!("{result:?}\nlen={}\n", prefixes.len())
                .replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_rejects_file_target_dir() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-file-dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"file-dir-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("tests"), "not a directory\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_rejects_file_target_dir",
            &format!("{result:?}\nlen={}\n", prefixes.len())
                .replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_ignores_rs_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-rs-directory");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"rs-dir-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests/fake.rs")).unwrap();
        std::fs::write(dir.join("tests/real.rs"), "fn main() {}\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_ignores_rs_directory",
            &format!("result={result:?}\n{output}\n"),
        );

        assert!(result.is_ok());
        assert!(prefixes.contains(&CrateName::from("real")));
        assert!(!prefixes.contains(&CrateName::from("fake")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_collect_auto_target_prefixes_rejects_whitespace_rs_stem() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-whitespace-rs-stem");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"whitespace-rs-stem-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/bad name.rs"), "fn main() {}\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = format!("result={result:?}\nlen={}\n", prefixes.len())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_collect_auto_target_prefixes_rejects_whitespace_rs_stem",
            &output,
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_collect_auto_target_prefixes_rejects_non_utf8_rs_stem() {
        use std::os::unix::ffi::OsStringExt as _;

        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-non-utf8-rs-stem");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"non-utf8-rs-stem-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        let file_name =
            std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff, b'.', b'r', b's']);
        std::fs::write(dir.join("tests").join(file_name), "fn main() {}\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = format!("is_err={}\nlen={}\n", result.is_err(), prefixes.len());
        rvs_snapshot_BIS(
            "test_20260707_collect_auto_target_prefixes_rejects_non_utf8_rs_stem",
            &output,
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_collect_auto_target_prefixes_rejects_non_utf8_dir_name() {
        use std::os::unix::ffi::OsStringExt as _;

        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-non-utf8-dir-name");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"non-utf8-dir-name-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        let dir_name = std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff]);
        let target_dir = dir.join("tests").join(dir_name);
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("main.rs"), "fn main() {}\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = format!("is_err={}\nlen={}\n", result.is_err(), prefixes.len());
        rvs_snapshot_BIS(
            "test_20260707_collect_auto_target_prefixes_rejects_non_utf8_dir_name",
            &output,
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_detect_local_crate_prefixes_for_cargo_check_excludes_unchecked_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("production-target-prefixes");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"prod-prefix-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[[test]]\nname = \"tokio\"\n\n[[example]]\nname = \"serde\"\n\n[[bench]]\nname = \"criterion\"\n\n[[bin]]\nname = \"tool-bin\"\npath = \"src/main.rs\"\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo_example.rs"),
            ("benches", "fast_bench.rs"),
            ("src/bin", "helper_bin.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }

        let prefixes = rvs_detect_local_crate_prefixes_for_cargo_check_BIS(&dir, false).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_detect_local_crate_prefixes_for_cargo_check_excludes_unchecked_targets",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("prod_prefix_demo")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(prefixes.contains(&CrateName::from("helper_bin")));
        assert!(!prefixes.contains(&CrateName::from("tokio")));
        assert!(!prefixes.contains(&CrateName::from("serde")));
        assert!(!prefixes.contains(&CrateName::from("criterion")));
        assert!(!prefixes.contains(&CrateName::from("ui_tests")));
        assert!(!prefixes.contains(&CrateName::from("demo_example")));
        assert!(!prefixes.contains(&CrateName::from("fast_bench")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_ensure_cargo_project_requires_cargo_toml() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-workspace-cargo-check-{}-{unique}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();

        let result = rvs_ensure_cargo_project_BIS(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_ensure_cargo_project_requires_cargo_toml",
            &output,
        );
        assert!(result.is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    fn rvs_command_env_value(cmd: &Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs().find_map(|(name, value)| {
            if name == key {
                Some(value.map(|v| v.to_string_lossy().into_owned()))
            } else {
                None
            }
        })
    }

    fn rvs_command_env_os_value(cmd: &Command, key: &str) -> Option<Option<OsString>> {
        cmd.get_envs().find_map(|(name, value)| {
            if name == key {
                Some(value.map(OsString::from))
            } else {
                None
            }
        })
    }

    #[test]
    fn test_20260704_prepare_cargo_check_sanitizes_rivus_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("sanitize-env-no-caps");
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let capsmap_state = match rvs_command_env_value(&cmd, "RIVUS_CAPSMAP") {
            Some(None) => "removed",
            Some(Some(path)) if Path::new(&path).is_absolute() => "absolute",
            Some(Some(_)) => "relative",
            None => "inherited",
        };
        let output = format!(
            "callgraph={:?}\ncapsmap={capsmap_state}\nrustc={:?}\nrivus_enabled={:?}\n",
            rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH"),
            rvs_command_env_value(&cmd, "RUSTC"),
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
        );
        rvs_snapshot_BIS(
            "test_20260704_prepare_cargo_check_sanitizes_rivus_env",
            &output,
        );

        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH_DIR"),
            Some(None)
        );
        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_CAPSMAP"), Some(None));
        assert_eq!(rvs_command_env_value(&cmd, "RUSTC"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
            Some(Some("1".to_string()))
        );
        assert_eq!(rvs_command_env_value(&cmd, "RUSTC_WRAPPER"), Some(None));
        assert!(matches!(
            rvs_command_env_value(&cmd, "RUSTC_WORKSPACE_WRAPPER"),
            Some(Some(_))
        ));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_extra_env_cannot_override_driver_control() {
        let dir = rvs_make_workspace_temp_dir_BIS("extra-env-driver-control");
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![
                ("RIVUS_ENABLED", "0".into()),
                ("RUSTC", "bad-rustc".into()),
                ("RUSTC_WRAPPER", "bad-unselected-wrapper".into()),
                ("RUSTC_WORKSPACE_WRAPPER", "bad-wrapper".into()),
            ],
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let enabled = rvs_command_env_value(&cmd, "RIVUS_ENABLED");
        let rustc = rvs_command_env_value(&cmd, "RUSTC");
        let unselected_wrapper = rvs_command_env_value(&cmd, "RUSTC_WRAPPER");
        let wrapper = rvs_command_env_value(&cmd, "RUSTC_WORKSPACE_WRAPPER");
        let output = format!(
            "enabled={enabled:?}\nrustc={rustc:?}\nunselected_wrapper={unselected_wrapper:?}\nwrapper_is_bad={}\n",
            wrapper == Some(Some("bad-wrapper".to_string()))
        );
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_extra_env_cannot_override_driver_control",
            &output,
        );

        assert_eq!(enabled, Some(Some("1".to_string())));
        assert_eq!(rustc, Some(None));
        assert_eq!(unselected_wrapper, Some(None));
        assert_ne!(wrapper, Some(Some("bad-wrapper".to_string())));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_callgraph_env_requires_one() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-env-zero");
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/ext"), "std::env::var=\n").unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![("RIVUS_CALLGRAPH", "0".into())],
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIMS(&config);
        let output = format!("result={result:?}\n");
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_callgraph_env_requires_one",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_callgraph_collection_env_driver_dir_wins_over_extra_env() {
        let env_vars = rvs_callgraph_collection_env(
            vec![("RIVUS_CALLGRAPH_DIR", OsString::from("bad-dir"))],
            OsString::from("real-dir"),
        );
        let final_dir = env_vars
            .iter()
            .rev()
            .find(|(key, _)| *key == "RIVUS_CALLGRAPH_DIR")
            .map(|(_, value)| value.to_string_lossy().into_owned());
        let callgraph_flag = env_vars
            .iter()
            .rev()
            .find(|(key, _)| *key == "RIVUS_CALLGRAPH")
            .map(|(_, value)| value.to_string_lossy().into_owned());
        let output = format!("final_dir={final_dir:?}\ncallgraph={callgraph_flag:?}\n");
        rvs_snapshot_BIS(
            "test_20260707_callgraph_collection_env_driver_dir_wins_over_extra_env",
            &output,
        );

        assert_eq!(final_dir.as_deref(), Some("real-dir"));
        assert_eq!(callgraph_flag.as_deref(), Some("1"));
    }

    #[test]
    fn test_20260705_prepare_cargo_check_preserves_extra_capsmap_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("preserve-extra-capsmap");
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let explicit_caps = dir.join("explicit-caps");
        std::fs::create_dir_all(&explicit_caps).unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![("RIVUS_CAPSMAP", explicit_caps.clone().into_os_string())],
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let capsmap = rvs_command_env_value(&cmd, "RIVUS_CAPSMAP")
            .and_then(|value| value)
            .expect("extra capsmap should be set");
        let output = format!(
            "capsmap={}\n",
            Path::new(&capsmap).file_name().unwrap().to_string_lossy()
        );
        rvs_snapshot_BIS(
            "test_20260705_prepare_cargo_check_preserves_extra_capsmap_env",
            &output,
        );

        assert_eq!(PathBuf::from(capsmap), explicit_caps);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_prepare_cargo_check_preserves_non_unicode_capsmap_env() {
        use std::os::unix::ffi::OsStringExt;

        let dir = rvs_make_workspace_temp_dir_BIS("non-unicode-extra-capsmap");
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let non_unicode_name = OsString::from_vec(vec![b'c', b'a', b'p', b's', 0xff]);
        let explicit_caps = dir.join(non_unicode_name);
        std::fs::create_dir_all(&explicit_caps).unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![("RIVUS_CAPSMAP", explicit_caps.clone().into_os_string())],
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let capsmap = rvs_command_env_os_value(&cmd, "RIVUS_CAPSMAP")
            .and_then(|value| value)
            .expect("capsmap env should be set");
        let output = format!(
            "preserved={}\nabsolute={}\n",
            capsmap == explicit_caps.as_os_str(),
            Path::new(&capsmap).is_absolute()
        );
        rvs_snapshot_BIS(
            "test_20260707_prepare_cargo_check_preserves_non_unicode_capsmap_env",
            &output,
        );

        assert_eq!(capsmap, explicit_caps.as_os_str());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_rejects_invalid_extra_capsmap_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("invalid-extra-capsmap");
        let explicit_caps = dir.join("explicit-caps");
        std::fs::create_dir_all(&explicit_caps).unwrap();
        std::fs::write(explicit_caps.join("ext"), "bad=Z\n").unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![("RIVUS_CAPSMAP", explicit_caps.into_os_string())],
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIMS(&config);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_rejects_invalid_extra_capsmap_env",
            &output,
        );

        assert!(matches!(result, Err(CargoCheckError::Message(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_normalizes_relative_extra_capsmap_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("relative-extra-capsmap");
        std::fs::create_dir_all(dir.join("explicit-caps")).unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![("RIVUS_CAPSMAP", "explicit-caps".into())],
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let capsmap = rvs_command_env_value(&cmd, "RIVUS_CAPSMAP")
            .and_then(|value| value)
            .expect("capsmap env should be set");
        let output = format!(
            "absolute={}\nends={}\n",
            Path::new(&capsmap).is_absolute(),
            capsmap.ends_with("explicit-caps")
        );
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_normalizes_relative_extra_capsmap_env",
            &output,
        );

        assert_eq!(PathBuf::from(capsmap), dir.join("explicit-caps"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_rejects_caps_file() {
        let dir = rvs_make_workspace_temp_dir_BIS("caps-file");
        std::fs::write(dir.join("caps"), "bad=Z\n").unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIMS(&config);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_rejects_caps_file",
            &output,
        );

        assert!(matches!(result, Err(CargoCheckError::Message(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_prepare_cargo_check_rejects_broken_project_caps_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("broken-project-caps-symlink");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"broken-caps-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(dir.join("missing-caps"), dir.join("caps")).unwrap();

        let result = rvs_prepare_cargo_check_command_BIMS(&CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: None,
        });
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_rejects_broken_project_caps_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("capsmap path must be a directory"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_load_project_caps_rejects_broken_caps_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("load-broken-project-caps-symlink");
        std::os::unix::fs::symlink(dir.join("missing-caps"), dir.join("caps")).unwrap();

        let result = rvs_load_project_caps_BIS(&dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_load_project_caps_rejects_broken_caps_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("capsmap path must be a directory"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_check_rejects_forwarded_manifest_path() {
        let equals_args = vec!["--manifest-path=other/Cargo.toml".to_string()];
        let split_args = vec![
            "--manifest-path".to_string(),
            "other/Cargo.toml".to_string(),
        ];
        let normal_args = vec!["--all-targets".to_string()];

        let equals = rvs_reject_forwarded_check_args(&equals_args);
        let split = rvs_reject_forwarded_check_args(&split_args);
        let normal = rvs_reject_forwarded_check_args(&normal_args);
        let output = format!("equals={equals:?}\nsplit={split:?}\nnormal={normal:?}\n");
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_forwarded_manifest_path",
            &output,
        );

        assert!(equals.is_err());
        assert!(split.is_err());
        assert!(normal.is_ok());
    }

    #[test]
    fn test_20260706_check_rejects_dangerous_forwarded_config() {
        let build_rustc = vec![
            "--config".to_string(),
            "build.rustc=\"clippy-driver\"".to_string(),
        ];
        let wrapper = vec!["--config=build.rustc-wrapper=\"/tmp/w\"".to_string()];
        let rivus_env = vec![
            "--config".to_string(),
            "env.RIVUS_ENABLED={ value=\"0\", force=true }".to_string(),
        ];
        let rustc_env = vec!["--config=env.RUSTC_WRAPPER.value=\"bad\"".to_string()];
        let path_config = vec!["--config".to_string(), "ci-cargo-config.toml".to_string()];
        let harmless = vec!["--config=net.offline=true".to_string()];

        let output = format!(
            "build_rustc={}\nwrapper={}\nrivus_env={}\nrustc_env={}\npath_config={}\nharmless={}\n",
            rvs_reject_forwarded_check_args(&build_rustc).is_err(),
            rvs_reject_forwarded_check_args(&wrapper).is_err(),
            rvs_reject_forwarded_check_args(&rivus_env).is_err(),
            rvs_reject_forwarded_check_args(&rustc_env).is_err(),
            rvs_reject_forwarded_check_args(&path_config).is_err(),
            rvs_reject_forwarded_check_args(&harmless).is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_dangerous_forwarded_config",
            &output,
        );

        assert!(rvs_reject_forwarded_check_args(&build_rustc).is_err());
        assert!(rvs_reject_forwarded_check_args(&wrapper).is_err());
        assert!(rvs_reject_forwarded_check_args(&rivus_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&rustc_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&path_config).is_err());
        assert!(rvs_reject_forwarded_check_args(&harmless).is_ok());
        assert!(rvs_reject_dangerous_forwarded_config("build.rustc=\"clippy-driver\"").is_err());
        assert!(rvs_reject_dangerous_forwarded_config("net.offline=true").is_ok());
    }

    #[test]
    fn test_20260706_check_rejects_dangerous_forwarded_config_toml_keys() {
        let spaced = "build . rustc = \"bad\"";
        let quoted = "\"build\".\"rustc-wrapper\"=\"bad\"";
        let mixed_quote = "env.'RUSTC_WRAPPER'.value=\"bad\"";
        let escaped = "env.\"RUSTC\\u005fWRAPPER\".value=\"bad\"";
        let harmless_quoted = "\"net\".\"offline\"=true";
        let output = format!(
            "spaced={}\nquoted={}\nmixed_quote={}\nescaped={}\nharmless_quoted={}\n",
            rvs_reject_dangerous_forwarded_config(spaced).is_err(),
            rvs_reject_dangerous_forwarded_config(quoted).is_err(),
            rvs_reject_dangerous_forwarded_config(mixed_quote).is_err(),
            rvs_reject_dangerous_forwarded_config(escaped).is_err(),
            rvs_reject_dangerous_forwarded_config(harmless_quoted).is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_dangerous_forwarded_config_toml_keys",
            &output,
        );

        assert!(rvs_reject_dangerous_forwarded_config(spaced).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(quoted).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(mixed_quote).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(escaped).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(harmless_quoted).is_ok());
        let doc = escaped.parse::<toml_edit::DocumentMut>().unwrap();
        let mut paths = Vec::new();
        for (key, item) in doc.iter() {
            rvs_collect_config_key_paths_M(key, item, &mut paths);
        }
        assert!(paths.iter().any(|path| path == "env.RUSTC_WRAPPER"));
    }

    #[test]
    fn test_20260706_clean_dir_removes_file_path() {
        let dir = rvs_make_workspace_temp_dir_BIS("clean-dir-file-path");
        let path = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "stale").unwrap();

        let result = rvs_clean_dir_BIS(&path);
        let exists = path.exists();
        let output = format!("result={result:?}\nexists={exists}\n");
        rvs_snapshot_BIS("test_20260706_clean_dir_removes_file_path", &output);

        assert!(result.is_ok());
        assert!(!exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_clean_dir_removes_broken_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("clean-dir-broken-symlink");
        let path = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(dir.join("missing"), &path).unwrap();

        let result = rvs_clean_dir_BIS(&path);
        let symlink_exists = std::fs::symlink_metadata(&path).is_ok();
        let output = format!("result={result:?}\nsymlink_exists={symlink_exists}\n");
        rvs_snapshot_BIS("test_20260706_clean_dir_removes_broken_symlink", &output);

        assert!(result.is_ok());
        assert!(!symlink_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_write_capsmap_result_rejects_output_directory_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-dir-preflight");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("out-dir");
        std::fs::create_dir_all(&output_path).unwrap();
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_output_directory_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_output_without_file_name() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-no-file-name");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output = Some(PathBuf::new());

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n",)
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_output_without_file_name",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_parent_dir_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-parent-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join("..");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_parent_dir_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_internal_parent_dir_without_side_effects() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-internal-parent-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join("..").join("deps");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let missing_exists = dir.join("missing").exists();
        let deps_exists = dir.join("deps").exists();
        let output = format!(
            "result={result:?}\ndefault_exists={default_exists}\nmissing_exists={missing_exists}\ndeps_exists={deps_exists}\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_internal_parent_dir_without_side_effects",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(!missing_exists);
        assert!(!deps_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_current_dir_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-current-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join(".");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_current_dir_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_trailing_slash_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-trailing-slash");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output = Some(PathBuf::from(format!("{}/out/", dir.to_string_lossy())));

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_trailing_slash_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_capsmap_output_paths_overlap_detects_equal_and_nested_paths() {
        let equal = rvs_capsmap_output_paths_overlap_BIS(
            Path::new("target/./caps"),
            Path::new("target/caps"),
        )
        .unwrap();
        let nested = rvs_capsmap_output_paths_overlap_BIS(
            Path::new("target/rivus-std-capsmap.txt"),
            Path::new("target/rivus-std-capsmap.txt/child"),
        )
        .unwrap();
        let sibling = rvs_capsmap_output_paths_overlap_BIS(
            Path::new("target/rivus-std-capsmap.txt"),
            Path::new("target/rivus-deps-capsmap.txt"),
        )
        .unwrap();
        let output = format!("equal={equal}\nnested={nested}\nsibling={sibling}\n");
        rvs_snapshot_BIS(
            "test_20260707_capsmap_output_paths_overlap_detects_equal_and_nested_paths",
            &output,
        );

        assert!(equal);
        assert!(nested);
        assert!(!sibling);
    }

    #[test]
    fn test_20260707_normalize_path_lexically_preserves_relative_parents() {
        let normalized =
            rvs_normalize_path_lexically(Path::new("/workspace/project/./caps/../target"));
        let normalized_root_parent = rvs_normalize_path_lexically(Path::new(
            "/../workspace/project/target/rivus-inferred-capsmap.txt",
        ));
        let normalized_leading_parents = rvs_normalize_path_lexically(Path::new("../../target"));
        let normalized_past_relative_root = rvs_normalize_path_lexically(Path::new("a/../../b"));
        let output = format!(
            "normalized={}\nnormalized_root_parent={}\nnormalized_leading_parents={}\nnormalized_past_relative_root={}\n",
            normalized.display(),
            normalized_root_parent.display(),
            normalized_leading_parents.display(),
            normalized_past_relative_root.display()
        );
        rvs_snapshot_BIS(
            "test_20260707_normalize_path_lexically_preserves_relative_parents",
            &output,
        );

        assert_eq!(normalized, PathBuf::from("/workspace/project/target"));
        assert_eq!(
            normalized_root_parent,
            PathBuf::from("/workspace/project/target/rivus-inferred-capsmap.txt")
        );
        assert_eq!(normalized_leading_parents, PathBuf::from("../../target"));
        assert_eq!(normalized_past_relative_root, PathBuf::from("../b"));
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_file_preserves_existing_temp_collision() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-temp-collision");
        let path = dir.join("deps");
        let temp_path = dir.join(format!(".deps.{}.0.tmp", std::process::id()));
        std::fs::write(&temp_path, "old-temp\n").unwrap();

        let result = rvs_write_capsmap_file_BIS(&path, "new=BI\n", "deps capsmap");
        let final_text = std::fs::read_to_string(&path).unwrap_or_default();
        let temp_text = std::fs::read_to_string(&temp_path).unwrap_or_default();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_file_preserves_existing_temp_collision",
            &format!("result={result:?}\nfinal={final_text:?}\ntemp={temp_text:?}\n"),
        );

        assert!(result.is_ok());
        assert_eq!(final_text, "new=BI\n");
        assert_eq!(temp_text, "old-temp\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_result_rejects_symlink_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-symlink");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_target = dir.join("output-target-dir");
        let output_path = dir.join("output-link");
        std::fs::create_dir_all(&output_target).unwrap();
        std::os::unix::fs::symlink(&output_target, &output_path).unwrap();
        let output = Some(output_path.clone());

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let symlink_exists = std::fs::symlink_metadata(&output_path).is_ok();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_symlink_output_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\nsymlink_exists={symlink_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(symlink_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_write_capsmap_result_rejects_socket_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-socket");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("output-socket");
        let listener = std::os::unix::net::UnixListener::bind(&output_path).unwrap();
        let output = Some(output_path.clone());

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let socket_exists = std::fs::symlink_metadata(&output_path).is_ok();
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_socket_output_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\nsocket_exists={socket_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(socket_exists);
        drop(listener);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_result_rejects_broken_parent_symlink_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-broken-parent-symlink");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let parent_link = dir.join("missing-parent-link");
        std::os::unix::fs::symlink(dir.join("missing-parent"), &parent_link).unwrap();
        let output = Some(parent_link.join("capsmap"));

        let result = rvs_write_capsmap_result_BIS(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_broken_parent_symlink_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_validates_project_caps_without_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("invalid-project-caps-no-std-cache");
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/ext"), "bad=Z\n").unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIMS(&config);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_validates_project_caps_without_std_cache",
            &output,
        );

        assert!(matches!(result, Err(CargoCheckError::Message(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_prepare_cargo_check_uses_absolute_paths() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let relative_project = PathBuf::from("target").join(format!(
            "rivus-relative-project-{}-{unique}",
            std::process::id()
        ));
        let absolute_project = std::env::current_dir().unwrap().join(&relative_project);
        std::fs::create_dir_all(absolute_project.join("caps")).unwrap();

        let config = CargoCheckConfig {
            project_path: &relative_project,
            wrap_all_crates: false,
            with_tests: true,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: Some("rivus-custom-build"),
        };

        let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
        let current_dir = cmd.get_current_dir().expect("command should set cwd");
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let target_dir = args
            .windows(2)
            .find_map(|window| (window[0] == "--target-dir").then(|| PathBuf::from(&window[1])))
            .expect("command should set target dir");
        let capsmap = rvs_command_env_value(&cmd, "RIVUS_CAPSMAP")
            .and_then(|value| value)
            .map(PathBuf::from)
            .expect("project caps should be configured");
        let output = format!(
            "cwd_abs={}\ntarget_abs={}\ncaps_abs={}\n",
            current_dir.is_absolute(),
            target_dir.is_absolute(),
            capsmap.is_absolute(),
        );
        rvs_snapshot_BIS(
            "test_20260704_prepare_cargo_check_uses_absolute_paths",
            &output,
        );

        assert!(current_dir.is_absolute());
        assert!(target_dir.is_absolute());
        assert!(capsmap.is_absolute());
        assert!(target_dir.ends_with("target/rivus-custom-build"));
        assert!(capsmap.ends_with("caps"));

        std::fs::remove_dir_all(absolute_project).unwrap();
    }

    #[test]
    fn test_20260703_load_callgraph_and_caps_includes_deps() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-workspace-load-callgraph-{}-{unique}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": ["std::thread::spawn"],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();
        std::fs::write(dir.join("caps/deps"), "std::thread::spawn=B\n").unwrap();

        let callgraph = rvs_load_or_collect_callgraph_with_collector_BIMS(
            &dir,
            CachedCallgraphMode::ProjectRequired,
            |_| Err("collector should not run when cache is valid".to_string()),
        )
        .unwrap();
        let caps = rvs_load_project_caps_BIS(&dir).unwrap();
        let output = format!(
            "calls={}\nhas_deps={}\n",
            callgraph.rvs_len(),
            caps.rvs_lookup("std::thread::spawn").is_some()
        );
        rvs_snapshot_BIS(
            "test_20260703_load_callgraph_and_caps_includes_deps",
            &output,
        );

        assert_eq!(callgraph.rvs_len(), 1);
        assert!(caps.rvs_lookup("std::thread::spawn").is_some());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_project_cache_filters_local_nodes_from_std_cache() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-workspace-merge-callgraph-{}-{unique}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": ["demo::rvs_local"],
    "has_body": true,
    "has_async": true,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": ["std::fs::read_to_string"],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": true,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let callgraph =
            rvs_load_or_collect_callgraph_BIMS(&dir, CachedCallgraphMode::ProjectRequired).unwrap();
        let node = callgraph
            .rvs_get("demo::rvs_run")
            .expect("merged callgraph should keep duplicate node");
        let output = format!(
            "calls={:?}\nhas_async={}\nhas_mut_param={}\n",
            node.calls, node.facts.has_async, node.facts.has_mut_param,
        );
        rvs_snapshot_BIS(
            "test_20260704_project_cache_filters_local_nodes_from_std_cache",
            &output,
        );

        assert_eq!(node.calls.len(), 1);
        assert!(node.calls.contains("demo::rvs_local"));
        assert!(node.facts.has_async);
        assert!(!node.facts.has_mut_param);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_load_std_only_cached_callgraph() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-workspace-std-only-callgraph-{}-{unique}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let callgraph =
            rvs_load_or_collect_callgraph_BIMS(&dir, CachedCallgraphMode::StdOnlyAllowed).unwrap();
        let has_std = callgraph.rvs_get("std::fs::rvs_read_BI").is_some();
        rvs_snapshot_BIS(
            "test_20260704_load_std_only_cached_callgraph",
            &format!("has_std={has_std}\nlen={}\n", callgraph.rvs_len()),
        );

        assert!(has_std);
        assert_eq!(callgraph.rvs_len(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_project_required_merges_std_like_cache_after_fresh_collection() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-project-required");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_or_collect_callgraph_with_collector_BIMS(
            &dir,
            CachedCallgraphMode::ProjectRequired,
            |_path| {
                let mut graph = FnGraph::rvs_new();
                graph.rvs_insert_M(
                    crate::symbols::DefPath::from("demo::rvs_fresh"),
                    crate::artifacts::FnNode::default(),
                );
                Ok(graph)
            },
        );
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260704_project_required_merges_std_like_cache_after_fresh_collection",
            &output,
        );

        assert!(result.is_ok());
        assert!(
            result
                .as_ref()
                .is_ok_and(|graph| graph.rvs_get("demo::rvs_fresh").is_some())
        );
        assert!(output.contains("demo::rvs_fresh"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_project_required_ignores_empty_project_cache_with_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-project-cache-std");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_or_collect_callgraph_with_collector_BIMS(
            &dir,
            CachedCallgraphMode::ProjectRequired,
            |_path| {
                let mut graph = FnGraph::rvs_new();
                graph.rvs_insert_M(
                    crate::symbols::DefPath::from("demo::rvs_fresh"),
                    crate::artifacts::FnNode::default(),
                );
                Ok(graph)
            },
        );
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260704_project_required_ignores_empty_project_cache_with_std_cache",
            &output,
        );

        assert!(result.is_ok());
        assert!(
            result
                .as_ref()
                .is_ok_and(|graph| graph.rvs_get("demo::rvs_fresh").is_some())
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_project_required_ignores_wrong_type_std_callgraph_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("project-cache-wrong-type-std");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(dir.join("target/rivus-callgraph-std"), "stale").unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/project.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_or_collect_callgraph_with_collector_BIMS(
            &dir,
            CachedCallgraphMode::ProjectRequired,
            |_path| Err("collector should not run".into()),
        );
        let has_project = result
            .as_ref()
            .is_ok_and(|graph| graph.rvs_get("demo::rvs_run").is_some());
        rvs_snapshot_BIS(
            "test_20260706_project_required_ignores_wrong_type_std_callgraph_cache",
            &format!("has_project={has_project}\n"),
        );

        assert!(has_project);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_std_only_mode_requires_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-missing-cache");

        let result = rvs_load_or_collect_callgraph_BIMS(&dir, CachedCallgraphMode::StdOnlyAllowed);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS("test_20260704_std_only_mode_requires_std_cache", &output);

        assert!(result.is_err());
        assert!(output.contains("run cargo rivus infer-std first"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_std_only_rejects_callgraph_cache_file_path() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-callgraph-cache-file");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target/rivus-callgraph-std"), "stale").unwrap();

        let result = rvs_load_or_collect_callgraph_BIMS(&dir, CachedCallgraphMode::StdOnlyAllowed);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_std_only_rejects_callgraph_cache_file_path",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("std callgraph cache must be a directory"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_std_only_mode_ignores_project_cache_without_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-project-cache-no-std");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_or_collect_callgraph_BIMS(&dir, CachedCallgraphMode::StdOnlyAllowed);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_std_only_mode_ignores_project_cache_without_std_cache",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("run cargo rivus infer-std first"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_project_cache_stale_schema_falls_back_to_collection() {
        let dir = rvs_make_workspace_temp_dir_BIS("stale-project-cache-fallback");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_or_collect_callgraph_with_collector_BIMS(
            &dir,
            CachedCallgraphMode::ProjectRequired,
            |_path| {
                let mut graph = FnGraph::rvs_new();
                graph.rvs_insert_M(
                    crate::symbols::DefPath::from("demo::rvs_fresh"),
                    crate::artifacts::FnNode::default(),
                );
                Ok(graph)
            },
        );
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260704_project_cache_stale_schema_falls_back_to_collection",
            &output,
        );

        assert!(result.is_ok());
        assert!(
            result
                .as_ref()
                .is_ok_and(|graph| graph.rvs_get("demo::rvs_fresh").is_some())
        );
        assert!(!output.contains("stale callgraph JSON lacks has_body"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_is_std_like_def_path() {
        let output = format!(
            "std={}\ncore={}\nalloc={}\ncompiler_builtins={}\nlocal={}\n",
            rvs_is_std_like_def_path("std::fs::read"),
            rvs_is_std_like_def_path("core::mem::drop"),
            rvs_is_std_like_def_path("alloc::vec::Vec::new"),
            rvs_is_std_like_def_path("compiler_builtins::mem::memcpy"),
            rvs_is_std_like_def_path("demo::rvs_run"),
        );
        rvs_snapshot_BIS("test_20260704_is_std_like_def_path", &output);

        assert!(rvs_is_std_like_def_path("std::fs::read"));
        assert!(rvs_is_std_like_def_path("core::mem::drop"));
        assert!(rvs_is_std_like_def_path("alloc::vec::Vec::new"));
        assert!(rvs_is_std_like_def_path("compiler_builtins::mem::memcpy"));
        assert!(!rvs_is_std_like_def_path("demo::rvs_run"));
    }

    #[test]
    fn test_20260705_std_like_query_matching_local_crate_uses_project_mode() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-local-crate");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"std\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let local_names = rvs_load_local_crate_prefixes_BIS(&dir).unwrap();
        let matches_local = rvs_function_matches_local_prefix("std::rvs_run", &local_names);
        let local_mode = rvs_cached_callgraph_mode_for_function_BIS(&dir, "std::rvs_run").unwrap();
        let real_std_mode =
            rvs_cached_callgraph_mode_for_function_BIS(&dir, "core::mem::drop").unwrap();
        let output = format!(
            "matches_local={matches_local}\nlocal_mode={local_mode:?}\nreal_std_mode={real_std_mode:?}\n",
        );
        rvs_snapshot_BIS(
            "test_20260705_std_like_query_matching_local_crate_uses_project_mode",
            &output,
        );

        assert!(matches_local);
        assert_eq!(local_mode, CachedCallgraphMode::ProjectRequired);
        assert_eq!(real_std_mode, CachedCallgraphMode::StdOnlyAllowed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_std_like_query_mode_reports_invalid_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"std\"\n").unwrap();

        let result = rvs_cached_callgraph_mode_for_function_BIS(&dir, "std::rvs_run");
        rvs_snapshot_BIS(
            "test_20260706_std_like_query_mode_reports_invalid_cargo_toml",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_merge_std_like_callgraph_filters_local_nodes() {
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("demo::rvs_run".into(), crate::artifacts::FnNode::default());
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M(
            "demo::rvs_stale".into(),
            crate::artifacts::FnNode::default(),
        );
        source.rvs_insert_M(
            "std::fs::read_to_string".into(),
            crate::artifacts::FnNode::default(),
        );

        rvs_merge_std_like_callgraph_M(&mut target, source);
        let output = format!(
            "has_local={}\nhas_std={}\nlen={}\n",
            target.rvs_get("demo::rvs_stale").is_some(),
            target.rvs_get("std::fs::read_to_string").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260704_merge_std_like_callgraph_filters_local_nodes",
            &output,
        );

        assert!(target.rvs_get("demo::rvs_stale").is_none());
        assert!(target.rvs_get("std::fs::read_to_string").is_some());
        assert_eq!(target.rvs_len(), 2);
    }

    #[test]
    fn test_20260705_merge_std_like_callgraph_skips_local_std_crate_nodes() {
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("std::rvs_run".into(), crate::artifacts::FnNode::default());
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M("std::rvs_stale".into(), crate::artifacts::FnNode::default());
        source.rvs_insert_M(
            "core::mem::drop".into(),
            crate::artifacts::FnNode::default(),
        );
        let local_names = BTreeSet::from([CrateName::from("std")]);

        rvs_merge_std_like_callgraph_with_local_prefixes_M(&mut target, source, &local_names);
        let output = format!(
            "has_local_stale={}\nhas_core={}\nlen={}\n",
            target.rvs_get("std::rvs_stale").is_some(),
            target.rvs_get("core::mem::drop").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260705_merge_std_like_callgraph_skips_local_std_crate_nodes",
            &output,
        );

        assert!(target.rvs_get("std::rvs_stale").is_none());
        assert!(target.rvs_get("core::mem::drop").is_some());
        assert_eq!(target.rvs_len(), 2);
    }

    #[test]
    fn test_20260704_reject_stale_callgraph_without_has_body() {
        let path = Path::new("target/rivus-callgraph/callgraph.json");
        let result = rvs_reject_stale_callgraph_json(
            path,
            r#"{
  "demo::rvs_trait_method_P": {
    "calls": [],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        );
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_reject_stale_callgraph_without_has_body",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("stale callgraph JSON lacks has_body"));
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_rejects_empty_artifact_dir() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-callgraph-dir");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_rejects_empty_artifact_dir",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_rejects_empty_graph_json() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        std::fs::write(cg_dir.join("demo-1.json"), "{}\n").unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_rejects_empty_graph_json",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_sorts_json_artifacts() {
        let dir = rvs_make_workspace_temp_dir_BIS("sorted-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        std::fs::write(cg_dir.join("z.json"), "not json\n").unwrap();
        std::fs::write(cg_dir.join("a.json"), "not json\n").unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_sorts_json_artifacts",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("a.json"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_ignores_json_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-json-directory");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(cg_dir.join("a.json")).unwrap();
        std::fs::write(
            cg_dir.join("b.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir);
        let ok = result
            .as_ref()
            .is_ok_and(|graph| graph.rvs_get("demo::rvs_run").is_some());
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_ignores_json_directory",
            &format!("ok={ok}\n"),
        );

        assert!(ok);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260703_cargo_check_error_exit_code() {
        let output = format!(
            "message={}\nexit={}\n",
            CargoCheckError::Message("oops".into()).rvs_exit_code(),
            CargoCheckError::ExitCode(101).rvs_exit_code()
        );
        rvs_snapshot_BIS("test_20260703_cargo_check_error_exit_code", &output);

        assert_eq!(CargoCheckError::Message("oops".into()).rvs_exit_code(), 1);
        assert_eq!(CargoCheckError::ExitCode(101).rvs_exit_code(), 101);
    }
}
