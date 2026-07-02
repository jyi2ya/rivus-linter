use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::artifacts::{self, FnBehavior};
use crate::capsmap::{self, CapsMap};

/// Resolve the capsmap path for the lint pass.
///
/// Priority: user-provided > project caps/ dir > built-in caps/ dir.
/// Note: target/rivus-inferred-capsmap.txt is NOT used here - it's a
/// partial snapshot from infer-capsmap, not a complete caps source.
fn rvs_resolve_capsmap_BIMS(
    cmd: &mut Command,
    user_capsmap: &[&Path],
    project_path: &Path,
    self_path: &Path,
) -> Result<(), String> {
    if let Some(path) = user_capsmap.first().copied() {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("current dir invalid: {e}"))?
                .join(path)
        };
        if !abs.is_dir() {
            return Err(format!(
                "capsmap path must be a directory: {}",
                abs.display()
            ));
        }
        cmd.env("RIVUS_CAPSMAP", abs);
        return Ok(());
    }

    let project_caps = project_path.join("caps");
    if project_caps.is_dir() {
        cmd.env("RIVUS_CAPSMAP", project_caps);
        return Ok(());
    }

    let built_in_caps = self_path.parent().and_then(|exe_dir| {
        exe_dir
            .parent()
            .and_then(|path| path.parent())
            .map(|root| root.join("caps"))
    });
    if let Some(dir) = built_in_caps.filter(|path| path.is_dir()) {
        cmd.env("RIVUS_CAPSMAP", dir);
    }
    Ok(())
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
    /// User-provided capsmap path (highest priority).
    pub(crate) user_capsmap: Option<&'a Path>,
    /// Extra environment variables to set.
    pub(crate) extra_env: Vec<(&'a str, String)>,
    /// Extra cargo check arguments.
    pub(crate) extra_args: Vec<&'a str>,
    /// Output subdirectory name under target/ (e.g. "rivus-build", "rivus-report-build").
    /// If None, uses default target/ directory.
    pub(crate) target_subdir: Option<&'a str>,
}

/// Runs `cargo check` with the rivus lint pass configured according to `config`.
/// Returns `Ok(())` on success, `Err(message)` on failure.
///
/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
pub(crate) fn rvs_run_cargo_check_impl_BIMS(config: &CargoCheckConfig) -> Result<(), String> {
    let self_path =
        env::current_exe().map_err(|e| format!("current executable path invalid: {e}"))?;
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

    let has_callgraph_env = config
        .extra_env
        .iter()
        .any(|(key, _)| *key == "RIVUS_CALLGRAPH");
    if !has_callgraph_env {
        let user_capsmap: Vec<&Path> = config.user_capsmap.into_iter().collect();
        rvs_resolve_capsmap_BIMS(&mut cmd, &user_capsmap, config.project_path, &self_path)?;
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
        .map_err(|e| format!("could not run cargo: {e}"))?
        .wait()
        .map_err(|e| format!("failed to wait for cargo: {e}"))?;
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
pub(crate) fn rvs_run_cargo_check_BIMS(
    capsmap: &Option<PathBuf>,
    extra_args: &[String],
) -> Result<(), i32> {
    let project_path = Path::new(".");
    let extra_args_ref: Vec<&str> = extra_args.iter().map(|arg| arg.as_str()).collect();
    match rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
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
    extra_env: Vec<(&str, String)>,
) -> Result<BTreeMap<String, FnBehavior>, String> {
    let suffix = if build_std { "-std" } else { "" };
    let cg_subdir = format!("rivus-callgraph{suffix}");
    let build_subdir = format!("rivus-build{suffix}");

    let cg_dir = path.join("target").join(&cg_subdir);
    let abs_cg_dir = std::env::current_dir()
        .map_err(|e| format!("current dir invalid: {e}"))?
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

    rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
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

fn rvs_load_or_collect_callgraph_BIMS(path: &Path) -> Result<BTreeMap<String, FnBehavior>, String> {
    let cg_dir = path.join("target").join("rivus-callgraph");
    let cg_std_dir = path.join("target").join("rivus-callgraph-std");

    if cg_dir.is_dir() || cg_std_dir.is_dir() {
        let mut merged = BTreeMap::new();
        if cg_dir.is_dir() {
            let cg = rvs_merge_callgraph_dir_BIS(&cg_dir)?;
            merged.extend(cg);
        }
        if cg_std_dir.is_dir() {
            let cg = rvs_merge_callgraph_dir_BIS(&cg_std_dir)?;
            merged.extend(cg);
        }
        Ok(merged)
    } else {
        eprintln!("(no cached callgraph found, collecting fresh...)");
        rvs_collect_callgraph_BIMS(path, false, true, vec![])
    }
}

/// Load callgraph and caps for a project, used by annotate, why, and similar
/// commands that need inferred capabilities.
///
/// Loads callgraph via `rvs_load_or_collect_callgraph_BIMS` and caps
/// from `caps/` (excluding `deps`) via `CapsMap::rvs_load_dir_excluding_BIS`.
pub(crate) fn rvs_load_callgraph_and_caps_BIMS(
    path: &Path,
) -> Result<(BTreeMap<String, FnBehavior>, capsmap::CapsMap), String> {
    let callgraph = rvs_load_or_collect_callgraph_BIMS(path)?;
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

pub(crate) fn rvs_write_capsmap_result_BIS(
    result: &str,
    default_path: &Path,
    output: &Option<PathBuf>,
    label: &str,
) -> Result<(), String> {
    std::fs::write(default_path, result)
        .map_err(|e| format!("cannot write {}: {e}", default_path.display()))?;
    match output.as_deref() {
        Some(path) => {
            std::fs::write(path, result)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            println!("Written {label} to {}", path.display());
        }
        None => print!("{result}"),
    }
    Ok(())
}

pub(crate) fn rvs_collect_local_crate_prefixes(toml: &str) -> Result<BTreeSet<String>, String> {
    let doc: toml_edit::DocumentMut = toml.parse().map_err(|e| format!("invalid TOML: {e}"))?;

    let mut prefixes = BTreeSet::new();
    if let Some(package_name) = doc
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
    {
        prefixes.insert(package_name.replace('-', "_"));
    }
    if let Some(lib_name) = doc
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(|name| name.as_str())
    {
        prefixes.insert(lib_name.replace('-', "_"));
    }
    if let Some(bins) = doc.get("bin").and_then(toml_edit::Item::as_array_of_tables) {
        for bin in bins {
            if let Some(name) = bin.get("name").and_then(|name| name.as_str()) {
                prefixes.insert(name.replace('-', "_"));
            }
        }
    }
    if prefixes.is_empty() {
        return Err(
            "Cargo.toml: missing local crate target ([package].name, [lib].name, or [[bin]].name)"
                .into(),
        );
    }
    Ok(prefixes)
}

pub(crate) fn rvs_detect_local_crate_prefixes_BIS(path: &Path) -> Result<BTreeSet<String>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    rvs_collect_local_crate_prefixes(&content).map_err(|e| format!("{}: {e}", cargo_toml.display()))
}

pub(crate) fn rvs_load_local_crate_prefixes_BIS(path: &Path) -> Result<BTreeSet<String>, String> {
    rvs_ensure_cargo_project_BIS(path)?;
    rvs_detect_local_crate_prefixes_BIS(path)
}

pub(crate) fn rvs_clean_dir_BIS(path: &Path) {
    if path.exists() {
        drop(std::fs::remove_dir_all(path));
    }
}

pub(crate) fn rvs_resolve_capsmap_path(project_dir: &Path, capsmap_path: &Path) -> PathBuf {
    if capsmap_path.is_absolute() {
        capsmap_path.to_path_buf()
    } else {
        project_dir.join(capsmap_path)
    }
}

fn rvs_merge_callgraph_dir_BIS(cg_dir: &Path) -> Result<BTreeMap<String, FnBehavior>, String> {
    let mut merged: BTreeMap<String, FnBehavior> = BTreeMap::new();
    let cg_entries =
        std::fs::read_dir(cg_dir).map_err(|e| format!("cannot read {}: {e}", cg_dir.display()))?;
    for entry in cg_entries {
        let entry = entry.map_err(|e| format!("readdir error: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let json_str = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let partial = artifacts::rvs_parse_callgraph_json_S(&json_str)?;
            for (func, behavior) in partial {
                merged.entry(func).or_default().rvs_merge_M(&behavior);
            }
        }
    }
    Ok(merged)
}

/// # Panics
///
/// Panics if `rustc -vV` cannot be executed or returns a non-zero exit status.
fn rvs_host_triple_BIMS() -> String {
    let default_host = "x86_64-unknown-linux-gnu";
    let Ok(output) = Command::new("rustc").arg("-vV").output() else {
        return default_host.into();
    };
    if !output.status.success() {
        return default_host.into();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            return host.trim().to_string();
        }
    }
    default_host.into()
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

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    #[test]
    fn test_20260630_collect_local_crate_prefixes_bin_name() {
        let input = "[package]\nname = \"rivus-linter\"\n\n[[bin]]\nname = \"cargo-rivus\"\npath = \"src/main.rs\"\n";
        let prefixes = rvs_collect_local_crate_prefixes(input).expect("prefixes should parse");
        let output = prefixes.iter().cloned().collect::<Vec<_>>().join("\n");
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
        let output = format!("{result:?}");
        rvs_snapshot_BIS(
            "test_20260702_ensure_cargo_project_requires_cargo_toml",
            &output,
        );
        assert!(result.is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_resolve_capsmap_path_relative_to_project() {
        let project_dir = Path::new("/workspace/project");
        let relative = Path::new("caps");
        let absolute = Path::new("/shared/caps");

        let resolved_relative = rvs_resolve_capsmap_path(project_dir, relative);
        let resolved_absolute = rvs_resolve_capsmap_path(project_dir, absolute);
        let output = format!(
            "relative={}\nabsolute={}",
            resolved_relative.display(),
            resolved_absolute.display()
        );
        rvs_snapshot_BIS(
            "test_20260702_resolve_capsmap_path_relative_to_project",
            &output,
        );

        assert_eq!(resolved_relative, PathBuf::from("/workspace/project/caps"));
        assert_eq!(resolved_absolute, PathBuf::from("/shared/caps"));
    }
}
