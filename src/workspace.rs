use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::artifacts::FnGraph;
use crate::callgraph_cache::{
    rvs_is_std_like_def_path, rvs_load_published_std_callgraph_cache_BIS,
    rvs_merge_callgraph_dir_BIS, rvs_merge_std_like_callgraph_M,
    rvs_merge_std_like_callgraph_with_local_prefixes_M,
};
use crate::capsmap::{self, CapsMap};
use crate::cargo_targets::{CargoTargetScope, rvs_detect_local_crate_prefixes_BIS};
#[cfg(test)]
use crate::cargo_targets::{
    rvs_collect_auto_target_prefixes_BIMS, rvs_collect_local_crate_prefixes,
    rvs_collect_local_crate_prefixes_for_targets, rvs_insert_manifest_crate_name_M,
};
use crate::fs_guard::rvs_render_atomic_write_failure;
use crate::function_classification::LocalScope;
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
    crate::fs_guard::rvs_validate_optional_dir_BIS(path, "capsmap path")
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
    /// Select the same Cargo target universe used for local crate discovery.
    pub(crate) target_scope: CargoTargetScope,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum CallgraphCollectionError {
    Cargo(CargoCheckError),
    Artifact(String),
}

#[derive(Debug)]
struct RivusRunGeneration {
    root: PathBuf,
    artifact_dir: PathBuf,
    target_subdir: String,
}

impl RivusRunGeneration {
    fn rvs_root(&self) -> &Path {
        &self.root
    }

    fn rvs_artifact_dir(&self) -> &Path {
        &self.artifact_dir
    }

    fn rvs_target_subdir(&self) -> &str {
        &self.target_subdir
    }
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

impl fmt::Display for CallgraphCollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cargo(error) => error.fmt(f),
            Self::Artifact(message) => f.write_str(message),
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
        "RIVUS_UI_TESTING",
        "RIVUS_UNTESTED_PATHS",
        "RIVUS_ENABLED",
        "RIVUS_WRAPPER",
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
        if *key == "RIVUS_UI_TESTING" {
            return Err(CargoCheckError::Message(
                "driver-controlled env RIVUS_UI_TESTING cannot be forwarded to cargo".into(),
            ));
        }
        cmd.env(key, val);
    }

    for key in [
        "RIVUS_ENABLED",
        "RIVUS_WRAPPER",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        cmd.env_remove(key);
    }

    cmd.env(wrapper_env, &self_path)
        .env("RIVUS_ENABLED", "1")
        .env("RIVUS_WRAPPER", "1");

    let has_callgraph_env = config
        .extra_env
        .iter()
        .any(|(key, value)| *key == "RIVUS_CALLGRAPH" && value == "1");
    let has_capsmap_env = config
        .extra_env
        .iter()
        .any(|(key, _)| *key == "RIVUS_CAPSMAP");
    let offline_caps_env = config
        .extra_env
        .iter()
        .any(|(key, value)| *key == "RIVUS_OFFLINE_CAPS" && value == "1");
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
    if !has_callgraph_env && !has_capsmap_env && !offline_caps_env {
        rvs_resolve_capsmap_BIMS(&mut cmd, &project_path).map_err(CargoCheckError::Message)?;
    }

    cmd.arg("check");
    if let Some(arg) = config.target_scope.rvs_cargo_check_arg() {
        cmd.arg(arg);
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
    let target_scope = CargoTargetScope::WithTestExampleBench;
    let extra_args_ref: Vec<&str> = extra_args.iter().map(|arg| arg.as_str()).collect();
    let caps = match rvs_load_project_caps_BIS(project_path) {
        Ok(caps) => caps,
        Err(e) => {
            eprintln!("offline caps check cannot load caps/: {e}");
            return Err(1);
        }
    };
    let local_crate_names = match rvs_load_local_crate_prefixes_BIS(project_path, target_scope) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("offline caps check cannot detect local crates: {e}");
            return Err(1);
        }
    };
    let callgraph_result = rvs_collect_callgraph_with_args_detailed_BIMS(
        project_path,
        false,
        false,
        target_scope,
        vec![],
        extra_args_ref.clone(),
        &local_crate_names,
    );
    let callgraph = match callgraph_result {
        Ok(callgraph) => callgraph,
        Err(error) if rvs_callgraph_failure_exit_code(&error).is_some() => {
            eprintln!("offline caps check unavailable: {error}");
            return Err(rvs_callgraph_failure_exit_code(&error)
                .expect("never: guarded callgraph cargo failure has an exit code"));
        }
        Err(e) => {
            eprintln!("offline caps check unavailable: {e}");
            if let Err(lint_error) =
                rvs_run_project_lints_BIMS(project_path, target_scope, &extra_args_ref, None)
            {
                eprintln!("{lint_error}");
                return Err(lint_error.rvs_exit_code());
            }
            return Err(1);
        }
    };
    let uncovered =
        crate::offline_caps::rvs_uncovered_test_functions(&callgraph, &local_crate_names);
    let lint_result = rvs_run_project_lints_BIMS(
        project_path,
        target_scope,
        &extra_args_ref,
        Some(&uncovered),
    );
    if let Err(error) = lint_result {
        eprintln!("{error}");
        return Err(error.rvs_exit_code());
    }

    let report = crate::offline_caps::rvs_check_offline_caps(&callgraph, &caps, &local_crate_names);
    if !report.rvs_is_empty() {
        print!("{report}");
    }
    if report.rvs_has_errors() {
        Err(1)
    } else {
        Ok(())
    }
}

fn rvs_run_project_lints_BIMS(
    project_path: &Path,
    target_scope: CargoTargetScope,
    extra_args: &[&str],
    uncovered: Option<&BTreeSet<crate::artifacts::FunctionIdentity>>,
) -> Result<(), CargoCheckError> {
    let generation =
        rvs_reserve_run_generation_BIS(project_path, "lint").map_err(CargoCheckError::Message)?;
    let lint_result = (|| {
        let mut extra_env = vec![("RIVUS_OFFLINE_CAPS", OsString::from("1"))];
        if let Some(functions) = uncovered {
            let path = rvs_write_untested_selection_BIS(generation.rvs_root(), functions)
                .map_err(CargoCheckError::Message)?;
            extra_env.push(("RIVUS_UNTESTED_PATHS", path.as_os_str().to_os_string()));
        }
        rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
            project_path,
            wrap_all_crates: false,
            target_scope,
            build_std: false,
            extra_env,
            extra_args: extra_args.to_vec(),
            target_subdir: Some(generation.rvs_target_subdir()),
        })
    })();
    let cleanup_result = rvs_cleanup_run_generation_BIS(&generation);
    match (lint_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(cleanup)) => Err(CargoCheckError::Message(cleanup)),
        (Err(error), Err(cleanup)) => {
            eprintln!("warning: additionally failed to clean lint generation: {cleanup}");
            Err(error)
        }
    }
}

fn rvs_write_untested_selection_BIS(
    generation_root: &Path,
    functions: &BTreeSet<crate::artifacts::FunctionIdentity>,
) -> Result<PathBuf, String> {
    let path = generation_root.join("untested-functions.json");
    let json = crate::artifacts::rvs_serialize_function_identities_json_S(functions)?;
    let temp_path_for_attempt =
        |attempt| generation_root.join(format!(".untested-functions.{attempt}.tmp"));
    crate::fs_guard::rvs_write_atomic_BIS(&path, json.as_bytes(), &temp_path_for_attempt).map_err(
        |failure| rvs_render_atomic_write_failure(failure, &path, "temp coverage selection", false),
    )?;
    Ok(path)
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
        if arg == "--target-dir" || arg.starts_with("--target-dir=") {
            return Err("cargo rivus check does not support forwarded --target-dir because check uses an isolated target directory".into());
        }
        if matches!(
            arg.as_str(),
            "--lib"
                | "--bins"
                | "--bin"
                | "--examples"
                | "--example"
                | "--tests"
                | "--test"
                | "--benches"
                | "--bench"
                | "--all-targets"
        ) || arg.starts_with("--bin=")
            || arg.starts_with("--example=")
            || arg.starts_with("--test=")
            || arg.starts_with("--bench=")
        {
            return Err(format!(
                "cargo rivus check does not support forwarded target selector '{arg}'; check analyzes its fixed all-targets universe"
            ));
        }
        if matches!(
            arg.as_str(),
            "--help" | "-h" | "--version" | "-V" | "--unit-graph" | "--build-plan"
        ) || arg == "--print"
            || arg.starts_with("--print=")
        {
            return Err(format!(
                "cargo rivus check does not support forwarded non-building mode '{arg}'"
            ));
        }
        if arg == "--message-format" {
            let Some(value) = extra_args.get(index + 1) else {
                return Err(
                    "cargo rivus check does not support empty forwarded --message-format".into(),
                );
            };
            rvs_reject_json_message_format(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--message-format=") {
            rvs_reject_json_message_format(value)?;
        }
        if matches!(arg.as_str(), "--workspace" | "--all" | "--exclude")
            || arg.starts_with("--exclude=")
            || arg == "--package"
            || arg.starts_with("--package=")
            || arg == "-p"
            || (arg.starts_with("-p") && arg.len() > 2)
        {
            return Err(format!(
                "cargo rivus check does not support forwarded workspace package selector '{arg}'; run from the package directory"
            ));
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

fn rvs_reject_json_message_format(value: &str) -> Result<(), String> {
    if value.split(',').any(|format| format.starts_with("json")) {
        return Err(format!(
            "cargo rivus check does not support forwarded JSON message format '{value}'"
        ));
    }
    Ok(())
}

fn rvs_reject_dangerous_forwarded_config(value: &str) -> Result<(), String> {
    let dangerous_keys = [
        "build.rustc",
        "build.rustc-wrapper",
        "build.rustc-workspace-wrapper",
        "env.RIVUS_ENABLED",
        "env.RIVUS_WRAPPER",
        "env.RIVUS_CAPSMAP",
        "env.RIVUS_CALLGRAPH",
        "env.RIVUS_CALLGRAPH_DIR",
        "env.RIVUS_OFFLINE_CAPS",
        "env.RIVUS_UI_TESTING",
        "env.RIVUS_UNTESTED_PATHS",
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
/// Reserves private artifact and Cargo target directories, runs `cargo check`
/// with the callgraph collection environment, and returns the merged graph.
///
/// Dependency and std inference wrap every crate. Project analysis uses the
/// workspace-only collector below so third-party warnings stay capped.
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
    target_scope: CargoTargetScope,
    extra_env: Vec<(&str, OsString)>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_BIMS(
        path,
        build_std,
        true,
        target_scope,
        extra_env,
        vec![],
        local_crate_names,
    )
}

pub(crate) fn rvs_collect_workspace_callgraph_BIMS(
    path: &Path,
    target_scope: CargoTargetScope,
    extra_env: Vec<(&str, OsString)>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_BIMS(
        path,
        false,
        false,
        target_scope,
        extra_env,
        vec![],
        local_crate_names,
    )
}

pub(crate) fn rvs_collect_callgraph_with_args_BIMS(
    path: &Path,
    build_std: bool,
    wrap_all_crates: bool,
    target_scope: CargoTargetScope,
    extra_env: Vec<(&str, OsString)>,
    extra_args: Vec<&str>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_detailed_BIMS(
        path,
        build_std,
        wrap_all_crates,
        target_scope,
        extra_env,
        extra_args,
        local_crate_names,
    )
    .map_err(|error| error.to_string())
}

fn rvs_collect_callgraph_with_args_detailed_BIMS(
    path: &Path,
    build_std: bool,
    wrap_all_crates: bool,
    target_scope: CargoTargetScope,
    extra_env: Vec<(&str, OsString)>,
    extra_args: Vec<&str>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, CallgraphCollectionError> {
    let purpose = if build_std {
        "callgraph-std"
    } else {
        "callgraph"
    };
    let generation = rvs_reserve_run_generation_BIS(path, purpose)
        .map_err(CallgraphCollectionError::Artifact)?;
    let env_vars = rvs_callgraph_collection_env(
        extra_env,
        generation.rvs_artifact_dir().as_os_str().to_os_string(),
    );

    let collection_result = rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates,
        target_scope,
        build_std,
        extra_env: env_vars,
        extra_args,
        target_subdir: Some(generation.rvs_target_subdir()),
    })
    .map_err(CallgraphCollectionError::Cargo)
    .and_then(|()| {
        rvs_merge_callgraph_dir_BIS(generation.rvs_artifact_dir(), local_crate_names)
            .map_err(CallgraphCollectionError::Artifact)
    });
    let cleanup_result = rvs_cleanup_run_generation_BIS(&generation);
    match (collection_result, cleanup_result) {
        (Ok(callgraph), Ok(())) => Ok(callgraph),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(CallgraphCollectionError::Artifact(cleanup)),
        (Err(error), Err(cleanup)) => {
            eprintln!("warning: additionally failed to clean callgraph generation: {cleanup}");
            Err(error)
        }
    }
}

fn rvs_reserve_run_generation_BIS(
    project_path: &Path,
    purpose: &str,
) -> Result<RivusRunGeneration, String> {
    debug_assert!(
        !purpose.is_empty(),
        "run generation purpose must not be empty"
    );
    debug_assert!(
        purpose
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "run generation purpose must be path-safe ASCII"
    );
    let project_path = rvs_absolute_path_BIS(project_path)?;
    let runs_dir = project_path.join("target/.rivus-runs");
    std::fs::create_dir_all(&runs_dir)
        .map_err(|error| format!("cannot create {}: {error}", runs_dir.display()))?;
    for attempt in 0..100usize {
        debug_assert!(attempt < 100, "run generation retry bound");
        let name = format!("{purpose}-{}-{attempt}", std::process::id());
        let root = runs_dir.join(&name);
        match std::fs::create_dir(&root) {
            Ok(()) => {
                let artifact_dir = root.join("artifacts");
                if let Err(error) = std::fs::create_dir(&artifact_dir) {
                    let cleanup = std::fs::remove_dir_all(&root).err();
                    let cleanup = cleanup
                        .map(|cleanup| {
                            format!("; additionally cannot remove generation: {cleanup}")
                        })
                        .unwrap_or_default();
                    return Err(format!(
                        "cannot create {}: {error}{cleanup}",
                        artifact_dir.display()
                    ));
                }
                let target_subdir = Path::new(".rivus-runs")
                    .join(&name)
                    .join("cargo-target")
                    .to_string_lossy()
                    .into_owned();
                return Ok(RivusRunGeneration {
                    root,
                    artifact_dir,
                    target_subdir,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot reserve run generation {}: {error}",
                    root.display()
                ));
            }
        }
    }
    Err(format!(
        "cannot reserve run generation in {}: too many collisions",
        runs_dir.display()
    ))
}

fn rvs_cleanup_run_generation_BIS(generation: &RivusRunGeneration) -> Result<(), String> {
    rvs_clean_dir_BIS(generation.rvs_root())
}

fn rvs_callgraph_failure_exit_code(error: &CallgraphCollectionError) -> Option<i32> {
    match error {
        CallgraphCollectionError::Cargo(error) => Some(error.rvs_exit_code()),
        CallgraphCollectionError::Artifact(_) => None,
    }
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

fn rvs_load_required_std_callgraph_cache_BIS(path: &Path) -> Result<FnGraph, String> {
    match rvs_load_published_std_callgraph_cache_BIS(path) {
        Ok(Some(cg)) => {
            let mut std_only = FnGraph::rvs_new();
            rvs_merge_std_like_callgraph_M(&mut std_only, cg);
            if std_only.rvs_is_empty() {
                return Err(
                    "published std callgraph cache contains no std-like functions; run cargo rivus infer-std first"
                        .into(),
                );
            }
            return Ok(std_only);
        }
        Ok(None) => {}
        Err(error) => return Err(format!("{error}; run cargo rivus infer-std first")),
    }
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");
    if crate::fs_guard::rvs_validate_optional_dir_BIS(&cg_std_dir, "std callgraph cache")? {
        let cg = rvs_merge_callgraph_dir_BIS(&cg_std_dir, &BTreeSet::new())
            .map_err(|e| format!("{e}; run cargo rivus infer-std first"))?;
        let mut std_only = FnGraph::rvs_new();
        rvs_merge_std_like_callgraph_M(&mut std_only, cg);
        if !std_only.rvs_is_empty() {
            return Ok(std_only);
        }
    }
    Err("std callgraph cache not found; run cargo rivus infer-std first".into())
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
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let callgraph = if rvs_should_use_required_std_cache(function, local_crate_names) {
        rvs_load_required_std_callgraph_cache_BIS(path)?
    } else {
        rvs_collect_project_callgraph_with_optional_std_cache_BIMS(
            path,
            target_scope,
            local_crate_names,
        )?
    };
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_should_use_required_std_cache(
    function: &str,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    rvs_is_std_like_def_path(function)
        && !LocalScope::rvs_new(local_crate_names).rvs_contains_str(function)
}

pub(crate) fn rvs_collect_callgraph_and_caps_BIMS(
    path: &Path,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let callgraph = rvs_collect_project_callgraph_with_optional_std_cache_BIMS(
        path,
        target_scope,
        local_crate_names,
    )?;
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_collect_project_callgraph_with_optional_std_cache_BIMS(
    path: &Path,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    let mut callgraph =
        rvs_collect_workspace_callgraph_BIMS(path, target_scope, vec![], local_crate_names)?;
    match rvs_load_published_std_callgraph_cache_BIS(path) {
        Ok(Some(std_graph)) => {
            rvs_merge_std_like_callgraph_with_local_prefixes_M(
                &mut callgraph,
                std_graph,
                local_crate_names,
            );
            return Ok(callgraph);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("warning: ignoring stale published std callgraph cache: {error}");
            return Ok(callgraph);
        }
    }
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");
    if rvs_warn_optional_dir_BIS(&cg_std_dir, "std callgraph cache") {
        match rvs_merge_callgraph_dir_BIS(&cg_std_dir, &BTreeSet::new()) {
            Ok(std_graph) => rvs_merge_std_like_callgraph_with_local_prefixes_M(
                &mut callgraph,
                std_graph,
                local_crate_names,
            ),
            Err(e) => eprintln!("warning: ignoring stale std callgraph cache: {e}"),
        }
    }
    Ok(callgraph)
}

fn rvs_warn_optional_dir_BIS(path: &Path, label: &str) -> bool {
    match crate::fs_guard::rvs_validate_optional_dir_BIS(path, label) {
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
    rvs_write_capsmap_file_BIS(output, result, label)?;
    println!("Written {label} to {}", output.display());
    Ok(())
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
        crate::fs_guard::rvs_validate_optional_dir_BIS(parent, &format!("{label} output parent"))?;
    }
    Ok(())
}

fn rvs_write_capsmap_file_BIS(path: &Path, result: &str, label: &str) -> Result<(), String> {
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
    let temp_path_for_attempt = |attempt| {
        path.with_file_name(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            attempt
        ))
    };
    crate::fs_guard::rvs_write_atomic_BIS(path, result.as_bytes(), &temp_path_for_attempt).map_err(
        |failure| rvs_render_atomic_write_failure(failure, path, "temp capsmap file", false),
    )
}

pub(crate) fn rvs_load_local_crate_prefixes_BIS(
    path: &Path,
    target_scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_ensure_cargo_project_BIS(path)?;
    rvs_detect_local_crate_prefixes_BIS(path, target_scope)
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
    use crate::test_support::{
        rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };

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
    fn test_20260710_callgraph_source_records_exact_workspace_member_base() {
        let workspace = rvs_make_workspace_temp_dir_BIS("source-provenance");
        let member = workspace.join("member");
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let member_text = "pub fn rvs_parse() -> u8 { 1 }\n";
        std::fs::write(member.join("src/lib.rs"), member_text).unwrap();

        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "pub fn workspace_decoy() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(member.join("member/src")).unwrap();
        std::fs::write(
            member.join("member/src/lib.rs"),
            "pub fn nested_decoy() {}\n",
        )
        .unwrap();

        let graph = rvs_collect_callgraph_BIMS(
            &member,
            false,
            CargoTargetScope::Production,
            vec![],
            &BTreeSet::from([CrateName::from("member")]),
        )
        .unwrap();
        let source = graph
            .rvs_get("member::rvs_parse")
            .and_then(|node| node.sources.first())
            .expect("member function should have source metadata");
        let normalized =
            crate::rename::rvs_normalize_source_for_project_BIS(source, &member).unwrap();
        let range = source.name_start as usize..source.name_end as usize;
        let recorded_name = member_text
            .get(range)
            .expect("source range should select the function name");
        let output = format!(
            "file={}\nbase={}\nrelative={}\nnormalized={}\nname={recorded_name}\n",
            source.file.display(),
            source
                .base
                .as_deref()
                .map_or("<none>".into(), |base| base.display().to_string()),
            source.file.is_relative(),
            normalized.file.display(),
        )
        .replace(&workspace.to_string_lossy().into_owned(), "$WORKSPACE");
        rvs_snapshot_BIS(
            "test_20260710_callgraph_source_records_exact_workspace_member_base",
            &output,
        );

        if source.file.is_relative() {
            assert!(source.base.is_some());
        }
        assert_eq!(
            normalized.file,
            member.join("src/lib.rs").canonicalize().unwrap()
        );
        assert_eq!(recorded_name, "rvs_parse");
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn test_20260714_check_accepts_const_only_crate() {
        let dir = rvs_make_workspace_temp_dir_BIS("const-only-check");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"const-only\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautobins = false\nautotests = false\nautoexamples = false\nautobenches = false\n\n[lib]\ntest = false\nbench = false\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub const ANSWER: u8 = 42;\n").unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nmissing_artifact={}\n",
            output.status.success(),
            stderr.contains("no callgraph JSON artifacts") || stderr.contains("contained no nodes")
        );
        rvs_snapshot_BIS("test_20260714_check_accepts_const_only_crate", &summary);

        assert!(output.status.success(), "{stderr}");
        assert!(!summary.contains("missing_artifact=true"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_project_callgraph_excludes_path_dependency_nodes() {
        let dir = rvs_make_workspace_temp_dir_BIS("project-callgraph-local-only");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn rvs_local() { fixture_dep::dependency_helper(); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "pub fn dependency_helper() {}\n",
        )
        .unwrap();

        let local = BTreeSet::from([CrateName::from("local-app")]);
        let graph = rvs_collect_project_callgraph_with_optional_std_cache_BIMS(
            &dir,
            CargoTargetScope::Production,
            &local,
        )
        .unwrap();
        let paths = graph
            .rvs_keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let output = paths.join("\n") + "\n";
        rvs_snapshot_BIS(
            "test_20260714_project_callgraph_excludes_path_dependency_nodes",
            &output,
        );

        assert!(paths.iter().any(|path| path == "local_app::rvs_local"));
        assert!(!paths.iter().any(|path| path.starts_with("fixture_dep::")));
        assert!(
            graph
                .rvs_get("local_app::rvs_local")
                .is_some_and(|node| node.calls.contains("fixture_dep::dependency_helper"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_merged_coverage_honors_lint_levels() {
        let dir = rvs_make_workspace_temp_dir_BIS("merged-coverage-lint-levels");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"coverage-levels\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![forbid(unfulfilled_lint_expectations)]\n#![deny(rivus::rvs_untested_good_fn)]\n\n/// Allowed uncovered function.\n#[allow(rivus::rvs_untested_good_fn)]\npub fn rvs_allowed() -> i32 { 1 }\n\n/// Expected uncovered function.\n#[expect(rivus::rvs_untested_good_fn)]\npub fn rvs_expected() -> i32 { 2 }\n\n/// Denied uncovered function.\npub fn rvs_denied() -> i32 { 3 }\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nallowed_reported={}\nexpect_unfulfilled={}\ndenied_reported={}\n",
            output.status.success(),
            stderr.contains("good fn 'rvs_allowed' not called by any test"),
            stderr.contains("unfulfilled_lint_expectations"),
            stderr.contains("good fn 'rvs_denied' not called by any test"),
        );
        rvs_snapshot_BIS("test_20260714_merged_coverage_honors_lint_levels", &summary);

        assert!(!output.status.success());
        assert!(!stderr.contains("good fn 'rvs_allowed' not called by any test"));
        assert!(!stderr.contains("unfulfilled_lint_expectations"));
        assert!(stderr.contains("good fn 'rvs_denied' not called by any test"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_callgraph_collection_fulfills_statement_expectations() {
        let dir = rvs_make_workspace_temp_dir_BIS("statement-expectation-collection");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"statement-expectation\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![forbid(unfulfilled_lint_expectations)]\n\nstruct Holder {\n    #[expect(rivus::rvs_borrowed_param)]\n    value: &'static String,\n}\n\npub fn rvs_statement_expectation() {\n    #[expect(rivus::rvs_error_swallow)]\n    let _ = Result::<(), ()>::Err(()).ok();\n}\n",
        )
        .unwrap();
        let local = BTreeSet::from([CrateName::from("statement-expectation")]);

        let result = rvs_collect_workspace_callgraph_BIMS(
            &dir,
            CargoTargetScope::Production,
            vec![],
            &local,
        );
        let graph_has_function = result.as_ref().is_ok_and(|graph| {
            graph
                .rvs_get("statement_expectation::rvs_statement_expectation")
                .is_some()
        });
        let output = format!(
            "result_ok={}\ngraph_has_function={graph_has_function}\n",
            result.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_collection_fulfills_statement_expectations",
            &output,
        );

        assert!(result.is_ok(), "{result:?}");
        assert!(graph_has_function);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_callgraph_collection_caps_source_deny_warnings() {
        let dir = rvs_make_workspace_temp_dir_BIS("collection-deny-warnings");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"collection-deny-warnings\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![deny(warnings)]\n#![allow(non_snake_case)]\n\npub fn rvs_value() -> i32 {\n    let unused = 1;\n    42\n}\n",
        )
        .unwrap();
        let local = BTreeSet::from([CrateName::from("collection-deny-warnings")]);

        let result = rvs_collect_workspace_callgraph_BIMS(
            &dir,
            CargoTargetScope::Production,
            vec![],
            &local,
        );
        let graph_has_function = result.as_ref().is_ok_and(|graph| {
            graph
                .rvs_get("collection_deny_warnings::rvs_value")
                .is_some()
        });
        let output = format!(
            "result_ok={}\ngraph_has_function={graph_has_function}\n",
            result.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_collection_caps_source_deny_warnings",
            &output,
        );

        assert!(result.is_ok(), "{result:?}");
        assert!(graph_has_function);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_merged_coverage_distinguishes_same_path_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("merged-coverage-target-identity");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"same-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![allow(non_snake_case)]\n\npub fn rvs_same() -> i32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_20260714_calls_library_same() {\n        assert_eq!(super::rvs_same(), 1);\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "#![allow(non_snake_case)]\n\n#[allow(dead_code)]\nfn rvs_bin_value() -> i32 { 1 }\n\npub fn rvs_same() -> i32 { rvs_bin_value() }\n\nfn main() {\n    let _ = rvs_same();\n}\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warning = "good fn 'rvs_same' not called by any test";
        let warning_count = stderr.matches(warning).count();
        let report_output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("report")
            .arg(&dir)
            .output()
            .unwrap();
        let report_stdout = String::from_utf8_lossy(&report_output.stdout);
        let report_counts_both = report_stdout.contains("Total: 2 functions, 2 lines");
        let summary = format!(
            "success={}\nwarning_count={warning_count}\nmerge_conflict={}\nreport_success={}\nreport_counts_both={report_counts_both}\n",
            output.status.success(),
            stderr.contains("conflicting ordinary definitions across Cargo targets"),
            report_output.status.success(),
        );
        rvs_snapshot_BIS(
            "test_20260714_merged_coverage_distinguishes_same_path_targets",
            &summary,
        );

        assert!(output.status.success());
        assert_eq!(warning_count, 1, "{stderr}");
        assert!(!stderr.contains("conflicting ordinary definitions across Cargo targets"));
        assert!(report_output.status.success(), "{report_stdout}");
        assert!(report_counts_both, "{report_stdout}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_unresolved_local_callable_does_not_cover_function() {
        let dir = rvs_make_workspace_temp_dir_BIS("coverage-local-callable");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-callable\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(internal_features)]\n#![allow(non_snake_case)]\n#![warn(rivus::rvs_untested_good_fn)]\n\n/// Production function intentionally left uncovered.\npub fn rvs_target() -> i32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_20260714_calls_local_callable() {\n        let rvs_target = || 2;\n        assert_eq!(rvs_target(), 2);\n    }\n}\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warning_count = stderr
            .matches("good fn 'rvs_target' not called by any test")
            .count();
        let summary = format!(
            "success={}\nwarning_count={warning_count}\n",
            output.status.success(),
        );
        rvs_snapshot_BIS(
            "test_20260714_unresolved_local_callable_does_not_cover_function",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        assert_eq!(warning_count, 1, "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_direct_driver_checks_local_test_coverage() {
        let dir = rvs_make_workspace_temp_dir_BIS("direct-driver-coverage");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"direct-coverage\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(internal_features)]\n#![allow(non_snake_case)]\n#![warn(rivus::rvs_untested_good_fn)]\n\n/// Function intentionally left uncovered.\npub fn rvs_uncovered() -> i32 { 1 }\n",
        )
        .unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            target_scope: CargoTargetScope::Production,
            build_std: false,
            extra_env: vec![],
            extra_args: vec![],
            target_subdir: Some("direct-driver-coverage"),
        };
        let output = rvs_prepare_cargo_check_command_BIMS(&config)
            .unwrap()
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reported = stderr.contains("good fn 'rvs_uncovered' not called by any test");
        let summary = format!(
            "success={}\nuntested_reported={reported}\n",
            output.status.success(),
        );
        rvs_snapshot_BIS(
            "test_20260714_direct_driver_checks_local_test_coverage",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        assert!(reported, "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260709_collect_local_crate_prefixes_rejects_invalid_target_names_table() {
        let cases = [
            (
                "empty",
                "[package]\nname = \"\"\n",
                "[[bin]]\nname = \"\"\n",
                "",
            ),
            (
                "whitespace",
                "[package]\nname = \"bad name\"\n",
                "[lib]\nname = \" bad\"\n",
                "bad\tname",
            ),
            (
                "pathy",
                "[package]\nname = \"bad/name\"\n",
                "[lib]\nname = \"bad\\\\name\"\n",
                "bad\0name",
            ),
        ];
        let mut output = String::new();
        for (name, first_input, second_input, helper_name) in cases {
            let first = rvs_collect_local_crate_prefixes(first_input);
            let second = rvs_collect_local_crate_prefixes(second_input);
            let mut prefixes = BTreeSet::new();
            let helper = rvs_insert_manifest_crate_name_M(&mut prefixes, "demo", helper_name);
            output.push_str(&format!(
                "{name}: first={first:?} second={second:?} helper={helper:?} len={}\n",
                prefixes.len()
            ));
            assert!(first.is_err(), "{name}");
            assert!(second.is_err(), "{name}");
            assert!(helper.is_err(), "{name}");
            assert!(prefixes.is_empty(), "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260709_collect_local_crate_prefixes_rejects_invalid_target_names_table",
            &output,
        );
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
        let prefixes =
            rvs_collect_local_crate_prefixes_for_targets(input, CargoTargetScope::Production)
                .unwrap();
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
    fn test_20260712_cargo_target_scope_selects_optional_targets() {
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
        let production =
            rvs_collect_local_crate_prefixes_for_targets(input, CargoTargetScope::Production)
                .expect("production targets should parse");
        let with_optional = rvs_collect_local_crate_prefixes_for_targets(
            input,
            CargoTargetScope::WithTestExampleBench,
        )
        .expect("all targets should parse");
        let rvs_render = |targets: &BTreeSet<CrateName>| {
            targets
                .iter()
                .map(CrateName::rvs_as_str)
                .collect::<Vec<_>>()
                .join(",")
        };
        let output = format!(
            "production={}\nwith_optional={}\n",
            rvs_render(&production),
            rvs_render(&with_optional),
        );
        rvs_snapshot_BIS(
            "test_20260712_cargo_target_scope_selects_optional_targets",
            &output,
        );

        assert_eq!(
            production,
            BTreeSet::from([CrateName::from("core_demo"), CrateName::from("tool_bin")])
        );
        assert_eq!(
            with_optional,
            BTreeSet::from([
                CrateName::from("core_demo"),
                CrateName::from("demo_example"),
                CrateName::from("integration_test"),
                CrateName::from("throughput_bench"),
                CrateName::from("tool_bin"),
            ])
        );
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

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
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

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
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

    #[cfg(unix)]
    #[test]
    fn test_20260709_collect_auto_target_prefixes_reject_invalid_names_table() {
        use std::os::unix::ffi::OsStringExt as _;

        let cases = [
            ("whitespace_rs_stem", "bad name.rs", None),
            (
                "non_utf8_rs_stem",
                "",
                Some(std::ffi::OsString::from_vec(vec![
                    b'b', b'a', b'd', 0xff, b'.', b'r', b's',
                ])),
            ),
            (
                "non_utf8_dir_name",
                "",
                Some(std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff])),
            ),
        ];
        let mut output = String::new();
        for (name, file_name, os_name) in cases {
            let dir = rvs_make_workspace_temp_dir_BIS(&format!("auto-target-{name}"));
            std::fs::write(
                dir.join("Cargo.toml"),
                "[package]\nname = \"invalid-target-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            std::fs::create_dir_all(dir.join("tests")).unwrap();
            match (name, os_name) {
                ("whitespace_rs_stem", _) => {
                    std::fs::write(dir.join("tests").join(file_name), "fn main() {}\n").unwrap();
                }
                ("non_utf8_rs_stem", Some(file_name)) => {
                    std::fs::write(dir.join("tests").join(file_name), "fn main() {}\n").unwrap();
                }
                ("non_utf8_dir_name", Some(dir_name)) => {
                    let target_dir = dir.join("tests").join(dir_name);
                    std::fs::create_dir_all(&target_dir).unwrap();
                    std::fs::write(target_dir.join("main.rs"), "fn main() {}\n").unwrap();
                }
                _ => unreachable!("case setup should be exhaustive"),
            }
            let mut prefixes = BTreeSet::new();
            let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
            output.push_str(&format!(
                "{name}: is_err={} len={}\n",
                result.is_err(),
                prefixes.len()
            ));
            assert!(result.is_err(), "{name}");
            assert!(prefixes.is_empty(), "{name}");
            std::fs::remove_dir_all(dir).unwrap();
        }
        rvs_snapshot_BIS(
            "test_20260709_collect_auto_target_prefixes_reject_invalid_names_table",
            &output,
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_collect_auto_target_prefixes_reports_first_sorted_error() {
        use std::os::unix::ffi::OsStringExt as _;

        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-sorted-error");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"sorted-error-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/z bad.rs"), "fn main() {}\n").unwrap();
        let non_utf8_name = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'r', b's']);
        std::fs::write(dir.join("tests").join(non_utf8_name), "fn main() {}\n").unwrap();

        let mut prefixes = BTreeSet::new();
        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = format!("{result:?}\nlen={}\n", prefixes.len())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260714_collect_auto_target_prefixes_reports_first_sorted_error",
            &output,
        );

        assert!(output.contains("not UTF-8"), "{output}");
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

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
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
        let dir = rvs_make_workspace_temp_dir_BIS("cargo-check");

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
    fn test_20260713_prepare_cargo_check_matches_target_scope() {
        let dir = rvs_make_workspace_temp_dir_BIS("target-scope-command");
        let mut output = String::new();
        for (name, target_scope) in [
            ("production", CargoTargetScope::Production),
            ("all_targets", CargoTargetScope::WithTestExampleBench),
        ] {
            let config = CargoCheckConfig {
                project_path: &dir,
                wrap_all_crates: false,
                target_scope,
                build_std: false,
                extra_env: vec![],
                extra_args: vec![],
                target_subdir: None,
            };
            let cmd = rvs_prepare_cargo_check_command_BIMS(&config).unwrap();
            let args = cmd
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            output.push_str(&format!("{name}={}\n", args.join(" ")));
        }
        rvs_snapshot_BIS(
            "test_20260713_prepare_cargo_check_matches_target_scope",
            &output,
        );

        assert_eq!(
            output,
            "production=check\nall_targets=check --all-targets\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_prepare_cargo_check_sanitizes_rivus_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("sanitize-env-no-caps");
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            "callgraph={:?}\ncapsmap={capsmap_state}\nrustc={:?}\nrivus_enabled={:?}\nui_testing={:?}\nuntested_paths={:?}\n",
            rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH"),
            rvs_command_env_value(&cmd, "RUSTC"),
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
            rvs_command_env_value(&cmd, "RIVUS_UI_TESTING"),
            rvs_command_env_value(&cmd, "RIVUS_UNTESTED_PATHS"),
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
        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_UI_TESTING"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_UNTESTED_PATHS"),
            Some(None)
        );
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
            Some(Some("1".to_string()))
        );
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_WRAPPER"),
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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

    #[test]
    fn test_20260713_prepare_offline_cargo_check_defers_caps_to_parent() {
        let dir = rvs_make_workspace_temp_dir_BIS("offline-caps-parent-snapshot");
        std::fs::write(dir.join("caps"), "bad=Z\n").unwrap();
        let config = CargoCheckConfig {
            project_path: &dir,
            wrap_all_crates: false,
            target_scope: CargoTargetScope::WithTestExampleBench,
            build_std: false,
            extra_env: vec![("RIVUS_OFFLINE_CAPS", "1".into())],
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIMS(&config);
        let capsmap_env = result
            .as_ref()
            .ok()
            .and_then(|command| rvs_command_env_value(command, "RIVUS_CAPSMAP"))
            .flatten();
        let output = format!(
            "result_is_ok={}\ncapsmap_env={capsmap_env:?}\n",
            result.is_ok()
        );
        rvs_snapshot_BIS(
            "test_20260713_prepare_offline_cargo_check_defers_caps_to_parent",
            &output,
        );

        assert!(result.is_ok());
        assert!(capsmap_env.is_none());
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
        let normal_args = vec!["--release".to_string()];

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
    fn test_20260714_check_rejects_forwarded_target_dir() {
        let split_args = vec!["--target-dir".to_string(), "custom-target".to_string()];
        let equals_args = vec!["--target-dir=custom-target".to_string()];
        let normal_args = vec!["--release".to_string()];
        let output = format!(
            "split={}\nequals={}\nnormal={}\n",
            rvs_reject_forwarded_check_args(&split_args).is_err(),
            rvs_reject_forwarded_check_args(&equals_args).is_err(),
            rvs_reject_forwarded_check_args(&normal_args).is_ok(),
        );
        rvs_snapshot_BIS("test_20260714_check_rejects_forwarded_target_dir", &output);

        assert!(rvs_reject_forwarded_check_args(&split_args).is_err());
        assert!(rvs_reject_forwarded_check_args(&equals_args).is_err());
        assert!(rvs_reject_forwarded_check_args(&normal_args).is_ok());
    }

    #[test]
    fn test_20260714_check_rejects_workspace_package_selectors() {
        let cases = [
            vec!["--workspace".to_string()],
            vec!["--all".to_string()],
            vec!["--package".to_string(), "member".to_string()],
            vec!["--package=member".to_string()],
            vec!["-p".to_string(), "member".to_string()],
            vec!["-pmember".to_string()],
            vec!["--exclude".to_string(), "member".to_string()],
            vec!["--exclude=member".to_string()],
        ];
        let output = cases
            .iter()
            .map(|args| {
                format!(
                    "{}={:?}",
                    args.join(" "),
                    rvs_reject_forwarded_check_args(args)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_workspace_package_selectors",
            &(output + "\n"),
        );

        assert!(
            cases
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
    }

    #[test]
    fn test_20260714_check_rejects_forwarded_target_selectors() {
        let cases = [
            vec!["--lib".to_string()],
            vec!["--bins".to_string()],
            vec!["--bin".to_string(), "tool".to_string()],
            vec!["--bin=tool".to_string()],
            vec!["--examples".to_string()],
            vec!["--example".to_string(), "demo".to_string()],
            vec!["--example=demo".to_string()],
            vec!["--tests".to_string()],
            vec!["--test".to_string(), "ui".to_string()],
            vec!["--test=ui".to_string()],
            vec!["--benches".to_string()],
            vec!["--bench".to_string(), "perf".to_string()],
            vec!["--bench=perf".to_string()],
            vec!["--all-targets".to_string()],
        ];
        let output = cases
            .iter()
            .map(|args| {
                format!(
                    "{}={}",
                    args.join(" "),
                    rvs_reject_forwarded_check_args(args).is_err()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_forwarded_target_selectors",
            &(output + "\n"),
        );

        assert!(
            cases
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
    }

    #[test]
    fn test_20260714_check_rejects_non_building_and_json_modes() {
        let rejected = [
            vec!["--help".to_string()],
            vec!["-h".to_string()],
            vec!["--version".to_string()],
            vec!["-V".to_string()],
            vec!["--unit-graph".to_string()],
            vec!["--build-plan".to_string()],
            vec!["--print".to_string(), "cfg".to_string()],
            vec!["--print=cfg".to_string()],
            vec!["--message-format".to_string(), "json".to_string()],
            vec!["--message-format=json-diagnostic-short".to_string()],
            vec!["--message-format=short,json".to_string()],
        ];
        let accepted = [
            vec!["--message-format".to_string(), "short".to_string()],
            vec!["--message-format=human".to_string()],
            vec!["--release".to_string()],
        ];
        let output = format!(
            "rejected={}\naccepted={}\n",
            rejected
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err()),
            accepted
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_ok()),
        );
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_non_building_and_json_modes",
            &output,
        );

        assert!(
            rejected
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
        assert!(
            accepted
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_ok())
        );
    }

    #[test]
    fn test_20260714_callgraph_compile_failure_skips_lint_fallback() {
        let compile_failure = CallgraphCollectionError::Cargo(CargoCheckError::ExitCode(101));
        let artifact_failure = CallgraphCollectionError::Artifact("missing artifact".to_string());
        let output = format!(
            "compile={:?}\nartifact={:?}\n",
            rvs_callgraph_failure_exit_code(&compile_failure),
            rvs_callgraph_failure_exit_code(&artifact_failure),
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_compile_failure_skips_lint_fallback",
            &output,
        );

        assert_eq!(rvs_callgraph_failure_exit_code(&compile_failure), Some(101));
        assert_eq!(rvs_callgraph_failure_exit_code(&artifact_failure), None);
    }

    #[test]
    fn test_20260714_grouped_banned_import_reports_once() {
        let dir = rvs_make_workspace_temp_dir_BIS("grouped-banned-import");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("anyhow/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"banned-import-group\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nanyhow = { path = \"anyhow\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n\nextern crate anyhow as anyhow_alias;\nuse anyhow::{Context, Error, Result};\n#[allow(rivus::rvs_banned_import)] use anyhow::Context as AllowedContext; use anyhow::Error as DeniedError;\n\nmacro_rules! import_anyhow { ($alias:ident) => { use anyhow::Context as $alias; } }\n#[allow(rivus::rvs_banned_import)]\nmod allowed_macro {\n    import_anyhow!(AllowedMacroContext);\n    const _: usize = core::mem::size_of::<AllowedMacroContext>();\n}\n\nimport_anyhow!(DeniedMacroContext);\n\nconst _: usize = core::mem::size_of::<(anyhow_alias::Error, Context, Error, Result, AllowedContext, DeniedError, DeniedMacroContext)>();\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("anyhow/Cargo.toml"),
            "[package]\nname = \"anyhow\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("anyhow/src/lib.rs"),
            "pub struct Context;\npub struct Error;\npub struct Result;\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warning_count = stderr
            .lines()
            .filter(|line| line.contains("warning: banned import: anyhow"))
            .count();
        let grouped_caret_width = stderr
            .lines()
            .skip_while(|line| !line.contains("use anyhow::{Context, Error, Result};"))
            .find(|line| line.contains('^'))
            .map(|line| line.chars().filter(|character| *character == '^').count())
            .unwrap_or(0);
        let allowed_macro_reported = stderr.contains("import_anyhow!(AllowedMacroContext)");
        let denied_macro_reported = stderr.contains("import_anyhow!(DeniedMacroContext)");
        let summary = format!(
            "success={}\nwarning_count={warning_count}\ngrouped_span_covers_statement={}\nallowed_macro_reported={allowed_macro_reported}\ndenied_macro_reported={denied_macro_reported}\n",
            output.status.success(),
            grouped_caret_width > "Context".len(),
        );
        rvs_snapshot_BIS("test_20260714_grouped_banned_import_reports_once", &summary);

        assert!(output.status.success(), "{stderr}");
        assert_eq!(warning_count, 4, "{stderr}");
        assert!(grouped_caret_width > "Context".len(), "{stderr}");
        assert!(!allowed_macro_reported, "{stderr}");
        assert!(denied_macro_reported, "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
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
        let ui_env = vec!["--config=env.RIVUS_UI_TESTING.value=\"1\"".to_string()];
        let coverage_env = vec!["--config=env.RIVUS_UNTESTED_PATHS.value=\"bad\"".to_string()];
        let rustc_env = vec!["--config=env.RUSTC_WRAPPER.value=\"bad\"".to_string()];
        let path_config = vec!["--config".to_string(), "ci-cargo-config.toml".to_string()];
        let harmless = vec!["--config=net.offline=true".to_string()];

        let output = format!(
            "build_rustc={}\nwrapper={}\nrivus_env={}\nui_env={}\ncoverage_env={}\nrustc_env={}\npath_config={}\nharmless={}\n",
            rvs_reject_forwarded_check_args(&build_rustc).is_err(),
            rvs_reject_forwarded_check_args(&wrapper).is_err(),
            rvs_reject_forwarded_check_args(&rivus_env).is_err(),
            rvs_reject_forwarded_check_args(&ui_env).is_err(),
            rvs_reject_forwarded_check_args(&coverage_env).is_err(),
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
        assert!(rvs_reject_forwarded_check_args(&ui_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&coverage_env).is_err());
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
            target_scope: CargoTargetScope::WithTestExampleBench,
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
    fn test_20260715_callgraph_generations_are_sibling_safe() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-generation-isolation");
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| rvs_reserve_run_generation_BIS(&dir, "callgraph").unwrap());
            let second = scope.spawn(|| rvs_reserve_run_generation_BIS(&dir, "callgraph").unwrap());
            (first.join().unwrap(), second.join().unwrap())
        });
        std::fs::write(second.rvs_root().join("sentinel"), "active\n").unwrap();

        let distinct = first.rvs_root() != second.rvs_root();
        let artifact_dirs_are_absolute =
            first.rvs_artifact_dir().is_absolute() && second.rvs_artifact_dir().is_absolute();
        let target_dirs_are_distinct = first.rvs_target_subdir() != second.rvs_target_subdir();
        rvs_cleanup_run_generation_BIS(&first).unwrap();
        let sibling_preserved = second.rvs_root().join("sentinel").is_file();
        let first_removed = !first.rvs_root().exists();
        rvs_cleanup_run_generation_BIS(&second).unwrap();
        let output = format!(
            "distinct={distinct}\nartifact_dirs_are_absolute={artifact_dirs_are_absolute}\ntarget_dirs_are_distinct={target_dirs_are_distinct}\nfirst_removed={first_removed}\nsibling_preserved={sibling_preserved}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_callgraph_generations_are_sibling_safe",
            &output,
        );

        assert!(distinct);
        assert!(artifact_dirs_are_absolute);
        assert!(target_dirs_are_distinct);
        assert!(first_removed);
        assert!(sibling_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_concurrent_callgraph_collections_do_not_mix_generations() {
        let dir = rvs_make_cargo_project_BIS(
            "concurrent-callgraph-generations",
            "concurrent-callgraph-generations",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub fn rvs_common() {}\n\n#[cfg(feature = \"first\")]\npub fn rvs_first() {}\n\n#[cfg(feature = \"second\")]\npub fn rvs_second() {}\n",
            )],
        );
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"concurrent-callgraph-generations\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nfirst = []\nsecond = []\n",
        )
        .unwrap();
        let local_crate_names =
            BTreeSet::from([CrateName::from("concurrent_callgraph_generations")]);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                rvs_collect_callgraph_with_args_BIMS(
                    &dir,
                    false,
                    false,
                    CargoTargetScope::Production,
                    vec![],
                    vec!["--features", "first"],
                    &local_crate_names,
                )
            });
            let second = scope.spawn(|| {
                rvs_collect_callgraph_with_args_BIMS(
                    &dir,
                    false,
                    false,
                    CargoTargetScope::Production,
                    vec![],
                    vec!["--features", "second"],
                    &local_crate_names,
                )
            });
            (
                first.join().unwrap().unwrap(),
                second.join().unwrap().unwrap(),
            )
        });
        let first_isolated = first
            .rvs_get("concurrent_callgraph_generations::rvs_first")
            .is_some()
            && first
                .rvs_get("concurrent_callgraph_generations::rvs_second")
                .is_none();
        let second_isolated = second
            .rvs_get("concurrent_callgraph_generations::rvs_second")
            .is_some()
            && second
                .rvs_get("concurrent_callgraph_generations::rvs_first")
                .is_none();
        let generations_remaining = std::fs::read_dir(dir.join("target/.rivus-runs"))
            .unwrap()
            .count();
        let output = format!(
            "first_isolated={first_isolated}\nsecond_isolated={second_isolated}\ngenerations_remaining={generations_remaining}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_concurrent_callgraph_collections_do_not_mix_generations",
            &output,
        );

        assert!(first_isolated);
        assert!(second_isolated);
        assert_eq!(generations_remaining, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_published_std_callgraph_precedes_legacy_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("published-std-callgraph-precedence");
        let legacy_dir = dir.join("target/rivus-callgraph-std");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let mut legacy = FnGraph::rvs_new();
        legacy.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_legacy"),
            crate::artifacts::FnNode::default(),
        );
        std::fs::write(
            legacy_dir.join("legacy.json"),
            crate::artifacts::rvs_serialize_callgraph_json_S(&legacy).unwrap(),
        )
        .unwrap();
        let mut published = FnGraph::rvs_new();
        published.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_published"),
            crate::artifacts::FnNode::default(),
        );
        crate::callgraph_cache::rvs_publish_std_callgraph_cache_BIS(&dir, &published).unwrap();

        let loaded = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let published_present = loaded.rvs_get("std::rvs_published").is_some();
        let legacy_present = loaded.rvs_get("std::rvs_legacy").is_some();
        let cache_is_file = dir.join("target/rivus-callgraph-std.json").is_file();
        let output = format!(
            "published_present={published_present}\nlegacy_present={legacy_present}\ncache_is_file={cache_is_file}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_published_std_callgraph_precedes_legacy_directory",
            &output,
        );

        assert!(published_present);
        assert!(!legacy_present);
        assert!(cache_is_file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_failed_std_callgraph_publish_preserves_previous_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("failed-std-callgraph-publish");
        let mut previous = FnGraph::rvs_new();
        previous.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_previous"),
            crate::artifacts::FnNode::default(),
        );
        crate::callgraph_cache::rvs_publish_std_callgraph_cache_BIS(&dir, &previous).unwrap();
        let target_dir = dir.join("target");
        for attempt in 0..100usize {
            std::fs::write(
                target_dir.join(format!(
                    ".rivus-callgraph-std.json.{}.{attempt}.tmp",
                    std::process::id()
                )),
                "collision\n",
            )
            .unwrap();
        }
        let mut replacement = FnGraph::rvs_new();
        replacement.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_replacement"),
            crate::artifacts::FnNode::default(),
        );

        let result =
            crate::callgraph_cache::rvs_publish_std_callgraph_cache_BIS(&dir, &replacement);
        let loaded = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let previous_present = loaded.rvs_get("std::rvs_previous").is_some();
        let replacement_present = loaded.rvs_get("std::rvs_replacement").is_some();
        let output = format!(
            "result_is_err={}\nprevious_present={previous_present}\nreplacement_present={replacement_present}\n",
            result.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260715_failed_std_callgraph_publish_preserves_previous_cache",
            &output,
        );

        assert!(result.is_err());
        assert!(previous_present);
        assert!(!replacement_present);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_std_callgraph_publish_replaces_previous_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("replace-std-callgraph-cache");
        let mut previous = FnGraph::rvs_new();
        previous.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_previous"),
            crate::artifacts::FnNode::default(),
        );
        crate::callgraph_cache::rvs_publish_std_callgraph_cache_BIS(&dir, &previous).unwrap();
        let mut replacement = FnGraph::rvs_new();
        replacement.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_replacement"),
            crate::artifacts::FnNode::default(),
        );

        crate::callgraph_cache::rvs_publish_std_callgraph_cache_BIS(&dir, &replacement).unwrap();
        let loaded = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let previous_present = loaded.rvs_get("std::rvs_previous").is_some();
        let replacement_present = loaded.rvs_get("std::rvs_replacement").is_some();
        let output = format!(
            "previous_present={previous_present}\nreplacement_present={replacement_present}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_std_callgraph_publish_replaces_previous_cache",
            &output,
        );

        assert!(!previous_present);
        assert!(replacement_present);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_load_std_only_cached_callgraph() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-callgraph");
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

        let callgraph = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
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
    fn test_20260704_std_only_mode_requires_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-missing-cache");

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
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

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
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

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
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
    fn test_20260704_is_std_like_def_path() {
        let cases = [
            ("std::fs::read", true),
            ("core::mem::drop", true),
            ("alloc::vec::Vec::new", true),
            ("compiler_builtins::mem::memcpy", true),
            ("demo::rvs_run", false),
            ("stdx::fs::read", false),
            ("corex::mem::drop", false),
            ("allocx::vec::Vec::new", false),
            ("compiler_builtinsx::mem::memcpy", false),
        ];
        let output = format!(
            "std={}\ncore={}\nalloc={}\ncompiler_builtins={}\nlocal={}\n",
            rvs_is_std_like_def_path("std::fs::read"),
            rvs_is_std_like_def_path("core::mem::drop"),
            rvs_is_std_like_def_path("alloc::vec::Vec::new"),
            rvs_is_std_like_def_path("compiler_builtins::mem::memcpy"),
            rvs_is_std_like_def_path("demo::rvs_run"),
        );
        rvs_snapshot_BIS("test_20260704_is_std_like_def_path", &output);

        for (path, expected) in cases {
            assert_eq!(rvs_is_std_like_def_path(path), expected, "{path}");
        }
    }

    #[test]
    fn test_20260710_std_like_query_matching_local_crate_uses_project_collection() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-local-crate");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"std\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let local_names =
            rvs_load_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
        let matches_local = LocalScope::rvs_new(&local_names).rvs_contains_str("std::rvs_run");
        let local_uses_std_cache = rvs_should_use_required_std_cache("std::rvs_run", &local_names);
        let real_std_uses_std_cache =
            rvs_should_use_required_std_cache("core::mem::drop", &local_names);
        let output = format!(
            "matches_local={matches_local}\nlocal_uses_std_cache={local_uses_std_cache}\nreal_std_uses_std_cache={real_std_uses_std_cache}\n",
        );
        rvs_snapshot_BIS(
            "test_20260710_std_like_query_matching_local_crate_uses_project_collection",
            &output,
        );

        assert!(matches_local);
        assert!(!local_uses_std_cache);
        assert!(real_std_uses_std_cache);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_std_like_query_mode_reports_invalid_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"std\"\n").unwrap();

        let result = crate::cargo_targets::rvs_detect_local_crate_prefixes_for_function_query_BIS(
            &dir,
            CargoTargetScope::WithTestExampleBench,
        );
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
    fn test_20260710_merge_std_like_callgraph_preserves_existing_node_merge() {
        let mut target_node = crate::artifacts::FnNode::default();
        target_node.calls.insert("core::rvs_target_call".into());
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("std::rvs_shared".into(), target_node);

        let mut source_node = crate::artifacts::FnNode::default();
        source_node.calls.insert("alloc::rvs_source_call".into());
        source_node.facts.has_async = true;
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M("std::rvs_shared".into(), source_node);
        source.rvs_insert_M(
            "demo::rvs_filtered".into(),
            crate::artifacts::FnNode::default(),
        );

        rvs_merge_std_like_callgraph_M(&mut target, source);
        let merged = target
            .rvs_get("std::rvs_shared")
            .expect("never: merged std node must exist");
        let calls = merged
            .calls
            .iter()
            .map(crate::symbols::DefPath::rvs_as_str)
            .collect::<Vec<_>>()
            .join(",");
        let output = format!(
            "calls={calls}\nhas_async={}\nhas_filtered={}\nlen={}\n",
            merged.facts.has_async,
            target.rvs_get("demo::rvs_filtered").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260710_merge_std_like_callgraph_preserves_existing_node_merge",
            &output,
        );

        assert_eq!(merged.calls.len(), 2);
        assert!(merged.facts.has_async);
        assert!(target.rvs_get("demo::rvs_filtered").is_none());
        assert_eq!(target.rvs_len(), 1);
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
        let result = crate::artifacts::rvs_parse_callgraph_json_S(
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

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_rejects_empty_artifact_dir",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_merge_callgraph_dir_accepts_empty_graph_json() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        let json = crate::artifacts::rvs_serialize_callgraph_json_S(&FnGraph::rvs_new()).unwrap();
        std::fs::write(cg_dir.join("demo-1.json"), json).unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
        let output = format!(
            "is_ok={}\nis_empty={}\n",
            result.is_ok(),
            result.as_ref().is_ok_and(FnGraph::rvs_is_empty)
        );
        rvs_snapshot_BIS(
            "test_20260714_merge_callgraph_dir_accepts_empty_graph_json",
            &output,
        );

        assert!(result.is_ok());
        assert!(result.as_ref().is_ok_and(FnGraph::rvs_is_empty));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_sorts_json_artifacts() {
        let dir = rvs_make_workspace_temp_dir_BIS("sorted-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        std::fs::write(cg_dir.join("z.json"), "not json\n").unwrap();
        std::fs::write(cg_dir.join("a.json"), "not json\n").unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
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

        let result =
            rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::from([CrateName::from("demo")]));
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
    fn test_20260710_callgraph_artifact_write_failure_fails_cargo_BIS() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-artifact-write-failure");
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"callgraph-write-failure\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/lib.rs"),
            "pub fn rvs_value() -> u32 { 1 }\n",
        )
        .unwrap();
        let artifact_path = dir.join("artifact-path-is-a-file");
        std::fs::write(&artifact_path, "blocker\n").unwrap();

        let output = Command::new(rvs_cargo_command_from_env_BS())
            .arg("check")
            .arg("--quiet")
            .current_dir(&project)
            .env_remove("RUSTC")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("RIVUS_CALLGRAPH")
            .env_remove("RIVUS_CALLGRAPH_DIR")
            .env_remove("RIVUS_OFFLINE_CAPS")
            .env_remove("RIVUS_UI_TESTING")
            .env_remove("RIVUS_UNTESTED_PATHS")
            .env_remove("RIVUS_WRAPPER")
            .env("RUSTC_WRAPPER", rvs_current_wrapper_exe_BIS().unwrap())
            .env("RIVUS_ENABLED", "1")
            .env("RIVUS_CALLGRAPH", "1")
            .env("RIVUS_CALLGRAPH_DIR", &artifact_path)
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let has_error = stderr.contains("error: cannot write rivus callgraph artifact:");
        let has_old_warning = stderr.contains("warning: cannot write rivus callgraph artifact:");
        let mentions_artifact_path = stderr.contains(&artifact_path.to_string_lossy().into_owned());
        let snapshot = format!(
            "success={}\nhas_error={has_error}\nhas_old_warning={has_old_warning}\nmentions_artifact_path={mentions_artifact_path}\n",
            output.status.success()
        );

        assert!(!output.status.success(), "{snapshot}");
        assert!(has_error, "{snapshot}");
        assert!(!has_old_warning, "{snapshot}");
        assert!(mentions_artifact_path, "{snapshot}");
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_write_failure_fails_cargo_BIS",
            &snapshot,
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_driver_capsmap_load_failure_is_fatal() {
        let dir = rvs_make_workspace_temp_dir_BIS("driver-capsmap-load-failure");
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("caps")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"capsmap-load-failure\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/lib.rs"),
            "pub fn rvs_value() -> u32 { 1 }\n",
        )
        .unwrap();
        std::fs::write(project.join("caps/seed"), "invalid capsmap line\n").unwrap();

        let output = Command::new(rvs_cargo_command_from_env_BS())
            .arg("check")
            .arg("--quiet")
            .current_dir(&project)
            .env_remove("RUSTC")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("RIVUS_CALLGRAPH")
            .env_remove("RIVUS_CALLGRAPH_DIR")
            .env_remove("RIVUS_OFFLINE_CAPS")
            .env_remove("RIVUS_UI_TESTING")
            .env_remove("RIVUS_UNTESTED_PATHS")
            .env_remove("RIVUS_WRAPPER")
            .env("RUSTC_WRAPPER", rvs_current_wrapper_exe_BIS().unwrap())
            .env("RIVUS_ENABLED", "1")
            .env("RIVUS_CAPSMAP", project.join("caps"))
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let has_error = stderr.contains("error: failed to load capsmap:");
        let has_warning_fallback = stderr.contains("warning: failed to load capsmap:");
        let snapshot = format!(
            "success={}\nhas_error={has_error}\nhas_warning_fallback={has_warning_fallback}\n",
            output.status.success()
        );
        rvs_snapshot_BIS(
            "test_20260714_driver_capsmap_load_failure_is_fatal",
            &snapshot,
        );

        assert!(!output.status.success(), "{snapshot}");
        assert!(has_error, "{snapshot}");
        assert!(!has_warning_fallback, "{snapshot}");
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
