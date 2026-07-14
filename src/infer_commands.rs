use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::callgraph_cache::{rvs_is_std_like_def_path, rvs_publish_std_callgraph_cache_BIS};
use crate::capsmap::{CapsMap, rvs_reserved_layer_name};
use crate::cargo_targets::{CargoTargetScope, rvs_detect_local_crate_prefixes_BIS};
use crate::function_classification::LocalScope;
use crate::inference::{
    PreparedInference, rvs_build_impl_index, rvs_collect_direct_external_deps, rvs_format_capsmap,
    rvs_format_unknown_callees, rvs_generate_trait_aliases, rvs_infer_caps_with_index,
    rvs_initial_caps, rvs_scope_port_methods_M,
};
use crate::symbols::{CapsMapKey, DefPath};
use crate::workspace::{
    rvs_collect_callgraph_BIMS, rvs_ensure_cargo_project_BIS, rvs_preflight_capsmap_file_BIS,
    rvs_validate_optional_capsmap_dir_BIS, rvs_write_capsmap_result_BIS,
};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_infer_capsmap_BIMPS(path: &Path, output: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let project_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;
    let target_scope = CargoTargetScope::Production;
    let local_crate_names = rvs_detect_local_crate_prefixes_BIS(&project_path, target_scope)?;

    let abs_seed = project_path.join("caps");
    let resolved_output = rvs_prepare_output_path_BIS(&project_path, output, "deps capsmap")?;
    let caps_dir_exists = rvs_validate_optional_capsmap_dir_BIS(&abs_seed)?;
    let output_layer = rvs_caps_output_layer_BIS(&abs_seed, &resolved_output, caps_dir_exists)?;
    rvs_require_inference_output_layer(output_layer.as_deref(), "deps", "infer-capsmap")?;
    let mut excluded_layers = vec![OsStr::new("deps")];
    if let Some(layer) = output_layer.as_deref()
        && layer != OsStr::new("deps")
    {
        excluded_layers.push(layer);
    }
    let seed = if caps_dir_exists {
        CapsMap::rvs_load_dir_excluding_names_BIS(&abs_seed, &excluded_layers)
            .map_err(|e| format!("caps: {e}"))?
    } else {
        CapsMap::rvs_new()
    };

    let mut callgraph = rvs_collect_callgraph_BIMS(
        &project_path,
        false,
        target_scope,
        vec![],
        &local_crate_names,
    )?;
    let inference = PreparedInference::rvs_prepare_M(&mut callgraph, &seed, &local_crate_names);
    let (direct_external_calls, unknown_callees) = rvs_collect_direct_external_deps(
        &callgraph,
        &local_crate_names,
        &seed,
        inference.rvs_inferred(),
        inference.rvs_impl_index(),
    );

    if !unknown_callees.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown_callees,
            "error: the following external functions have no capability data.\n\
             Add them to caps/seed or caps/ext with the correct capability markers:\n\n",
        ));
    }

    let deps_result = rvs_format_capsmap(&direct_external_calls);
    rvs_write_capsmap_result_BIS(&deps_result, &resolved_output, "deps capsmap")
}

pub(crate) fn rvs_run_infer_std_BIMPS(path: &Path, output: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let project_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;
    let target_scope = CargoTargetScope::Production;
    let local_crate_names = rvs_detect_local_crate_prefixes_BIS(&project_path, target_scope)?;
    let local_scope = LocalScope::rvs_new(&local_crate_names);

    let caps_dir = project_path.join("caps");
    let caps_dir_exists = rvs_validate_optional_capsmap_dir_BIS(&caps_dir)?;
    let resolved_output = rvs_prepare_output_path_BIS(&project_path, output, "std capsmap")?;
    let output_layer = rvs_caps_output_layer_BIS(&caps_dir, &resolved_output, caps_dir_exists)?;
    rvs_require_inference_output_layer(output_layer.as_deref(), "std", "infer-std")?;
    let seed = CapsMap::rvs_load_dir_layers_BIS(&caps_dir, &["seed", "suppress"])
        .map_err(|e| format!("caps: {e}"))?;
    let mut callgraph = rvs_collect_callgraph_BIMS(
        &project_path,
        true,
        target_scope,
        vec![],
        &local_crate_names,
    )?;
    rvs_scope_port_methods_M(&mut callgraph, &local_crate_names);
    rvs_require_complete_std_collection(&callgraph, &local_scope)?;

    let pre_index = rvs_build_impl_index(&callgraph);
    let pre_inferred = rvs_initial_caps(&callgraph, &seed);
    let std_pre_inferred: BTreeMap<DefPath, crate::capability::CapabilitySet> = pre_inferred
        .iter()
        .filter(|(k, _)| rvs_is_std_like_def_path(k.rvs_as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut alias_seed = seed.clone();
    let pre_aliases = rvs_generate_trait_aliases(&std_pre_inferred, &pre_index, &callgraph);
    alias_seed.rvs_extend_entries_M(
        pre_aliases
            .into_iter()
            .map(|(key, caps)| (CapsMapKey::from(key), caps)),
    );

    let mut inferred = rvs_infer_caps_with_index(&callgraph, &alias_seed, &pre_index);
    let std_inferred: BTreeMap<DefPath, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(k, _)| rvs_is_std_like_def_path(k.rvs_as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let post_aliases = rvs_generate_trait_aliases(&std_inferred, &pre_index, &callgraph);
    inferred.extend(post_aliases);

    let std_only: BTreeMap<DefPath, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(name, _)| {
            !local_scope.rvs_contains(name) && rvs_is_std_like_def_path(name.rvs_as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let unknown = rvs_collect_std_unknown_callees(&callgraph, &inferred, &seed, &local_scope);

    if !unknown.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown,
            "error: the following functions are called by std but have no capability data.\n\
             Add them to caps/seed with the correct capability markers:\n\n",
        ));
    }

    let result = rvs_format_capsmap(&std_only);
    rvs_write_capsmap_result_BIS(&result, &resolved_output, "std capsmap")?;
    rvs_publish_std_callgraph_cache_BIS(&project_path, &callgraph)
}

fn rvs_require_inference_output_layer(
    output_layer: Option<&OsStr>,
    expected_layer: &str,
    command: &str,
) -> Result<(), String> {
    let Some(output_layer) = output_layer else {
        return Ok(());
    };
    if let Some(reserved_layer) = rvs_reserved_layer_name(output_layer)
        && reserved_layer != expected_layer
    {
        return Err(format!(
            "{command} output cannot replace reserved caps layer '{}'; expected '{expected_layer}'",
            output_layer.to_string_lossy()
        ));
    }
    Ok(())
}

fn rvs_require_complete_std_collection(
    callgraph: &crate::artifacts::FnGraph,
    local_scope: &LocalScope,
) -> Result<(), String> {
    let mut has_std = false;
    let mut has_core = false;
    let mut has_alloc = false;
    for (name, _) in callgraph.rvs_iter() {
        if local_scope.rvs_contains(name) {
            continue;
        }
        has_std |= name.rvs_as_str().starts_with("std::");
        has_core |= name.rvs_as_str().starts_with("core::");
        has_alloc |= name.rvs_as_str().starts_with("alloc::");
    }
    let missing = [("std", has_std), ("core", has_core), ("alloc", has_alloc)]
        .into_iter()
        .filter_map(|(name, present)| (!present).then_some(name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "infer-std collection missing non-local crate graphs: {}; refusing to replace std capsmap",
            missing.join(", ")
        ))
    }
}

fn rvs_prepare_output_path_BIS(
    project_path: &Path,
    output_path: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let resolved = rvs_resolve_output_path(project_path, output_path);
    rvs_preflight_capsmap_file_BIS(&resolved, label)?;
    Ok(resolved)
}

fn rvs_resolve_output_path(project_path: &Path, output_path: &Path) -> PathBuf {
    if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        project_path.join(output_path)
    }
}

fn rvs_caps_output_layer_BIS(
    caps_dir: &Path,
    output_path: &Path,
    caps_dir_exists: bool,
) -> Result<Option<OsString>, String> {
    let Some(parent) = output_path.parent() else {
        return Ok(None);
    };
    if !caps_dir_exists {
        let parent_targets_caps = if parent == caps_dir {
            true
        } else {
            let names_match = parent
                .file_name()
                .and_then(OsStr::to_str)
                .zip(caps_dir.file_name().and_then(OsStr::to_str))
                .is_some_and(|(parent_name, caps_name)| {
                    parent_name.eq_ignore_ascii_case(caps_name)
                });
            let roots_match = match (parent.parent(), caps_dir.parent()) {
                (Some(parent_root), Some(caps_root)) if parent_root == caps_root => true,
                (Some(parent_root), Some(caps_root))
                    if parent_root.is_dir() && caps_root.is_dir() =>
                {
                    same_file::is_same_file(parent_root, caps_root).map_err(|error| {
                        format!(
                            "cannot compare capsmap output root '{}' with project root '{}': {error}",
                            parent_root.display(),
                            caps_root.display()
                        )
                    })?
                }
                _ => false,
            };
            names_match && roots_match
        };
        return Ok(parent_targets_caps
            .then(|| output_path.file_name().map(OsStr::to_os_string))
            .flatten());
    }
    if !parent.is_dir() {
        return Ok(None);
    }
    let parent_is_caps = same_file::is_same_file(parent, caps_dir).map_err(|error| {
        format!(
            "cannot compare capsmap output parent '{}' with caps directory '{}': {error}",
            parent.display(),
            caps_dir.display()
        )
    })?;
    if !parent_is_caps {
        return Ok(None);
    }

    if !output_path.is_file() {
        return Ok(output_path.file_name().map(OsStr::to_os_string));
    }
    let entries = std::fs::read_dir(caps_dir).map_err(|error| {
        format!(
            "cannot read caps directory '{}': {error}",
            caps_dir.display()
        )
    })?;
    let mut identity_matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "cannot read caps directory '{}': {error}",
                caps_dir.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "cannot inspect caps layer '{}': {error}",
                entry.path().display()
            )
        })?;
        if !file_type.is_file() {
            continue;
        }
        let entry_name = entry.file_name();
        if same_file::is_same_file(entry.path(), output_path).map_err(|error| {
            format!(
                "cannot compare caps layer '{}' with output '{}': {error}",
                entry.path().display(),
                output_path.display()
            )
        })? {
            identity_matches.push(entry_name);
        }
    }
    identity_matches.sort();
    match identity_matches.as_slice() {
        [entry_name] => Ok(Some(entry_name.clone())),
        [] => Err(format!(
            "cannot identify existing capsmap output '{}' in caps directory '{}'",
            output_path.display(),
            caps_dir.display()
        )),
        entries => Err(format!(
            "capsmap output '{}' has multiple layer names in '{}': {entries:?}",
            output_path.display(),
            caps_dir.display()
        )),
    }
}

fn rvs_collect_std_unknown_callees(
    callgraph: &crate::artifacts::FnGraph,
    inferred: &BTreeMap<DefPath, crate::capability::CapabilitySet>,
    seed: &CapsMap,
    local_scope: &LocalScope,
) -> BTreeMap<DefPath, BTreeSet<DefPath>> {
    let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
    for (func, behavior) in callgraph.rvs_iter() {
        let is_std = rvs_is_std_like_def_path(func.rvs_as_str());
        let is_local = local_scope.rvs_contains(func);
        if !is_std || is_local {
            continue;
        }
        for callee in behavior.rvs_dependency_calls() {
            let callee_is_emitted_std = rvs_is_std_like_def_path(callee.rvs_as_str());
            if seed.rvs_lookup(callee.rvs_as_str()).is_some()
                || (callee_is_emitted_std && inferred.contains_key(callee))
            {
                continue;
            }
            unknown
                .entry(callee.clone())
                .or_default()
                .insert(func.clone());
        }
    }
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };

    #[test]
    fn test_20260702_infer_capsmap_rejects_workspace_root() {
        let dir = rvs_make_temp_dir_BIS("infer-capsmap-workspace-root");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/deps"));
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_infer_capsmap_rejects_workspace_root",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("missing local crate target"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_infer_std_rejects_workspace_root() {
        let dir = rvs_make_temp_dir_BIS("infer-std-workspace-root");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, Path::new("caps/std"));
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260702_infer_std_rejects_workspace_root", &output);

        assert!(result.is_err());
        assert!(output.contains("missing local crate target"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_infer_std_rejects_caps_file() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-std-caps-file",
            "infer-std-caps-file",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::write(dir.join("caps"), "bad=Z\n").unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, Path::new("caps/std"));
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260706_infer_std_rejects_caps_file", &output);

        assert!(result.is_err());
        assert!(output.contains("capsmap path must be a directory"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_infer_std_local_prefixes_exclude_unchecked_std_named_tests() {
        let dir = rvs_make_temp_dir_BIS("infer-std-local-prefix-targets");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[[test]]\nname = \"std\"\n\n[[example]]\nname = \"core\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/alloc.rs"), "fn main() {}\n").unwrap();

        let local_crate_names =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
        let output = local_crate_names
            .iter()
            .map(|name| name.rvs_prefix().rvs_as_str().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_infer_std_local_prefixes_exclude_unchecked_std_named_tests",
            &output,
        );

        assert!(
            local_crate_names
                .iter()
                .any(|name| name.rvs_as_str() == "demo")
        );
        assert!(
            !local_crate_names
                .iter()
                .any(|name| name.rvs_as_str() == "std")
        );
        assert!(
            !local_crate_names
                .iter()
                .any(|name| name.rvs_as_str() == "core")
        );
        assert!(
            !local_crate_names
                .iter()
                .any(|name| name.rvs_as_str() == "alloc")
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_infer_std_rejects_broken_caps_symlink() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-std-broken-caps-symlink",
            "infer-std-broken-caps",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::os::unix::fs::symlink(dir.join("missing-caps"), dir.join("caps")).unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, Path::new("caps/std"));
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_infer_std_rejects_broken_caps_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("capsmap path must be a directory"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_infer_std_rejects_invalid_seed_before_callgraph() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-std-invalid-seed-preflight",
            "infer-std-invalid-seed",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/seed"), "demo=Z\n").unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, Path::new("caps/std"));
        let callgraph_exists = dir.join("target/rivus-callgraph-std").exists();
        let output = format!("result={result:?}\ncallgraph_exists={callgraph_exists}\n",)
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_infer_std_rejects_invalid_seed_before_callgraph",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_infer_std_rejects_output_directory_before_callgraph() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-std-output-dir-preflight",
            "infer-std-output-dir",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let output_path = dir.join("std-output");
        std::fs::create_dir_all(&output_path).unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, &output_path);
        let callgraph_exists = dir.join("target/rivus-callgraph-std").exists();
        let output = format!("result={result:?}\ncallgraph_exists={callgraph_exists}\n",)
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260713_infer_std_rejects_output_directory_before_callgraph",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_infer_capsmap_rejects_reserved_seed_output() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-reserved-seed-output",
            "infer-capsmap-reserved-seed-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let sentinel = "manual::entry=B\n";
        std::fs::write(dir.join("caps/seed"), sentinel).unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/seed"));
        let callgraph_exists = dir.join("target/rivus-callgraph").exists();
        let seed_preserved = std::fs::read_to_string(dir.join("caps/seed")).unwrap() == sentinel;
        let output = format!(
            "result={result:?}\ncallgraph_exists={callgraph_exists}\nseed_preserved={seed_preserved}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_infer_capsmap_rejects_reserved_seed_output",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        assert!(seed_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_infer_capsmap_rejects_case_aliased_missing_caps_seed() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-case-aliased-missing-caps",
            "infer-capsmap-case-aliased-missing-caps",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("CAPS/seed"));
        let callgraph_exists = dir.join("target/rivus-callgraph").exists();
        let output_exists = dir.join("CAPS/seed").exists();
        let output = format!(
            "result={result:?}\ncallgraph_exists={callgraph_exists}\noutput_exists={output_exists}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_infer_capsmap_rejects_case_aliased_missing_caps_seed",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        assert!(!output_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_caps_output_layer_rejects_multiple_hardlink_names() {
        let dir = rvs_make_temp_dir_BIS("caps-output-hardlink-alias");
        let caps_dir = dir.join("caps");
        std::fs::create_dir_all(&caps_dir).unwrap();
        std::fs::write(caps_dir.join("seed"), "manual=B\n").unwrap();
        std::fs::hard_link(caps_dir.join("seed"), caps_dir.join("alias")).unwrap();

        let result = rvs_caps_output_layer_BIS(&caps_dir, &caps_dir.join("alias"), true);
        let output = format!("is_err={}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260715_caps_output_layer_rejects_multiple_hardlink_names",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_infer_std_rejects_reserved_deps_output_before_seed() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-std-reserved-deps-output",
            "infer-std-reserved-deps-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/seed"), "broken=Z\n").unwrap();
        let sentinel = "dependency::entry=I\n";
        std::fs::write(dir.join("caps/deps"), sentinel).unwrap();

        let result = rvs_run_infer_std_BIMPS(&dir, Path::new("caps/deps"));
        let callgraph_exists = dir.join("target/rivus-callgraph-std").exists();
        let deps_preserved = std::fs::read_to_string(dir.join("caps/deps")).unwrap() == sentinel;
        let output = format!(
            "result={result:?}\ncallgraph_exists={callgraph_exists}\ndeps_preserved={deps_preserved}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_infer_std_rejects_reserved_deps_output_before_seed",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        assert!(deps_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_infer_std_rejects_incomplete_std_collection() {
        let empty = crate::artifacts::FnGraph::rvs_new();
        let mut compiler_and_local_only = crate::artifacts::FnGraph::rvs_new();
        compiler_and_local_only.rvs_insert_M(
            DefPath::from("std::rvs_local"),
            crate::artifacts::FnNode::default(),
        );
        compiler_and_local_only.rvs_insert_M(
            DefPath::from("compiler_builtins::mem::copy"),
            crate::artifacts::FnNode::default(),
        );
        let mut core_only = crate::artifacts::FnGraph::rvs_new();
        core_only.rvs_insert_M(
            DefPath::from("core::mem::drop"),
            crate::artifacts::FnNode::default(),
        );
        let mut complete = core_only.clone();
        complete.rvs_insert_M(
            DefPath::from("std::fs::read"),
            crate::artifacts::FnNode::default(),
        );
        complete.rvs_insert_M(
            DefPath::from("alloc::vec::Vec::new"),
            crate::artifacts::FnNode::default(),
        );
        let no_locals = LocalScope::rvs_new(&BTreeSet::new());
        let local_std =
            LocalScope::rvs_new(&BTreeSet::from([crate::symbols::CrateName::from("std")]));
        let output = format!(
            "empty={:?}\ncompiler_and_local_only={:?}\ncore_only={:?}\ncomplete={:?}\n",
            rvs_require_complete_std_collection(&empty, &no_locals),
            rvs_require_complete_std_collection(&compiler_and_local_only, &local_std),
            rvs_require_complete_std_collection(&core_only, &no_locals),
            rvs_require_complete_std_collection(&complete, &no_locals),
        );
        rvs_snapshot_BIS(
            "test_20260715_infer_std_rejects_incomplete_std_collection",
            &output,
        );

        assert!(rvs_require_complete_std_collection(&empty, &no_locals).is_err());
        assert!(rvs_require_complete_std_collection(&compiler_and_local_only, &local_std).is_err());
        assert!(rvs_require_complete_std_collection(&core_only, &no_locals).is_err());
        assert!(rvs_require_complete_std_collection(&complete, &no_locals).is_ok());
    }

    #[test]
    fn test_20260715_inference_output_layer_roles() {
        let cases = [
            ("infer-capsmap", "deps", None, false),
            ("infer-capsmap", "deps", Some("deps"), false),
            ("infer-capsmap", "deps", Some("DEPS"), false),
            ("infer-capsmap", "deps", Some("generated"), false),
            ("infer-capsmap", "deps", Some("std"), true),
            ("infer-capsmap", "deps", Some("seed"), true),
            ("infer-capsmap", "deps", Some("SEED"), true),
            ("infer-capsmap", "deps", Some("suppress"), true),
            ("infer-capsmap", "deps", Some("ext"), true),
            ("infer-std", "std", None, false),
            ("infer-std", "std", Some("std"), false),
            ("infer-std", "std", Some("STD"), false),
            ("infer-std", "std", Some("generated"), false),
            ("infer-std", "std", Some("deps"), true),
            ("infer-std", "std", Some("DEPS"), true),
            ("infer-std", "std", Some("seed"), true),
            ("infer-std", "std", Some("suppress"), true),
            ("infer-std", "std", Some("ext"), true),
        ];
        let mut output = String::new();
        for (command, expected, layer, should_error) in cases {
            let result =
                rvs_require_inference_output_layer(layer.map(OsStr::new), expected, command);
            output.push_str(&format!(
                "{command}:{}={}\n",
                layer.unwrap_or("outside"),
                if result.is_err() { "error" } else { "ok" }
            ));
            assert_eq!(result.is_err(), should_error);
        }
        rvs_snapshot_BIS("test_20260715_inference_output_layer_roles", &output);
    }

    #[test]
    fn test_20260706_infer_capsmap_rejects_output_directory_before_cache_write() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-output-dir-preflight",
            "infer-capsmap-output-dir",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let output_path = dir.join("out-dir");
        std::fs::create_dir_all(&output_path).unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, &output_path);
        let cache_exists = dir.join("target/rivus-inferred-capsmap.txt").exists();
        let output = format!(
            "result_is_err={}\ncache_exists={cache_exists}\n",
            result.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260706_infer_capsmap_rejects_output_directory_before_cache_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!cache_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_infer_capsmap_rejects_dotdot_output_before_callgraph() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-dotdot-output",
            "infer-capsmap-dotdot-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let output_path = PathBuf::from("target/../target/rivus-inferred-capsmap.txt");

        let result = rvs_run_infer_capsmap_BIMPS(&dir, &output_path);
        let callgraph_exists = dir.join("target/rivus-callgraph").exists();
        let cache_exists = dir.join("target/rivus-inferred-capsmap.txt").exists();
        let output = format!(
            "result={result:?}\ncallgraph_exists={callgraph_exists}\ncache_exists={cache_exists}\n",
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_infer_capsmap_rejects_dotdot_output_before_callgraph",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        assert!(!cache_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_infer_capsmap_rejects_invalid_seed_before_callgraph() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-seed-preflight",
            "infer-capsmap-invalid-seed",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/seed"), "demo=Z\n").unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/deps"));
        let callgraph_exists = dir.join("target/rivus-callgraph").exists();
        let output = format!("result={result:?}\ncallgraph_exists={callgraph_exists}\n",)
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_infer_capsmap_rejects_invalid_seed_before_callgraph",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260710_infer_capsmap_replaces_invalid_deps() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-old-deps",
            "infer-capsmap-invalid-old-deps",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/deps"), "broken=Z\n").unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/deps"));
        assert!(
            result.is_ok(),
            "old deps output should be replaceable: {result:?}"
        );
        let deps = std::fs::read_to_string(dir.join("caps/deps")).unwrap();
        let output = format!("result={result:?}\ndeps={deps:?}\n");
        rvs_snapshot_BIS("test_20260710_infer_capsmap_replaces_invalid_deps", &output);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_infer_capsmap_replaces_invalid_custom_output() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-custom-output",
            "infer-capsmap-invalid-custom-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/generated"), "broken=Z\n").unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/generated"));
        assert!(
            result.is_ok(),
            "old custom output should not become its own seed: {result:?}"
        );
        let generated = std::fs::read_to_string(dir.join("caps/generated")).unwrap();
        let output = format!("result={result:?}\ngenerated={generated:?}\n");
        rvs_snapshot_BIS(
            "test_20260714_infer_capsmap_replaces_invalid_custom_output",
            &output,
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_infer_capsmap_replaces_invalid_output_through_symlink() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-symlink-output",
            "infer-capsmap-invalid-symlink-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/generated"), "broken=Z\n").unwrap();
        std::os::unix::fs::symlink(dir.join("caps"), dir.join("caps-alias")).unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps-alias/generated"));
        let generated = std::fs::read_to_string(dir.join("caps/generated")).unwrap();
        let output = format!("result={result:?}\ngenerated={generated:?}\n");
        rvs_snapshot_BIS(
            "test_20260714_infer_capsmap_replaces_invalid_output_through_symlink",
            &output,
        );

        assert!(result.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_infer_capsmap_replaces_invalid_non_utf8_output() {
        use std::os::unix::ffi::OsStringExt as _;

        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-non-utf8-output",
            "infer-capsmap-invalid-non-utf8-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        let layer = std::ffi::OsString::from_vec(vec![b'g', 0x80]);
        let output_path = PathBuf::from("caps").join(&layer);
        std::fs::write(dir.join(&output_path), "broken=Z\n").unwrap();

        let result = rvs_run_infer_capsmap_BIMPS(&dir, &output_path);
        let generated = std::fs::read_to_string(dir.join(&output_path)).unwrap();
        let output = format!("result={result:?}\ngenerated={generated:?}\n");
        rvs_snapshot_BIS(
            "test_20260714_infer_capsmap_replaces_invalid_non_utf8_output",
            &output,
        );

        assert!(result.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_infer_capsmap_creates_missing_caps_dir() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-missing-caps-dir",
            "infer-capsmap-missing-caps-dir",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps/deps"));
        let output_exists = dir.join("caps/deps").is_file();
        let output = format!("result={result:?}\noutput_exists={output_exists}\n");
        rvs_snapshot_BIS(
            "test_20260714_infer_capsmap_creates_missing_caps_dir",
            &output,
        );

        assert!(result.is_ok());
        assert!(output_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_resolve_output_path_relative_to_project() {
        let project = Path::new("/workspace/project");
        let relative = rvs_resolve_output_path(project, Path::new("caps/deps"));
        let absolute = rvs_resolve_output_path(project, Path::new("/shared/deps"));
        let output = format!(
            "relative={}\nabsolute={}\n",
            relative.display(),
            absolute.display(),
        );
        rvs_snapshot_BIS(
            "test_20260704_resolve_output_path_relative_to_project",
            &output,
        );

        assert_eq!(relative, PathBuf::from("/workspace/project/caps/deps"));
        assert_eq!(absolute, PathBuf::from("/shared/deps"));
    }

    #[test]
    fn test_20260704_collect_std_unknown_callees_reports_non_emitted_support_crate() {
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut std_node = crate::artifacts::FnNode::default();
        std_node
            .calls
            .insert(DefPath::from("support_crate::rvs_help_BI"));
        callgraph.rvs_insert_M(DefPath::from("std::fs::read_to_string"), std_node);
        callgraph.rvs_insert_M(
            DefPath::from("support_crate::rvs_help_BI"),
            crate::artifacts::FnNode::default(),
        );
        let inferred = BTreeMap::from([
            (
                DefPath::from("std::fs::read_to_string"),
                crate::capability::CapabilitySet::rvs_from_validated("BI"),
            ),
            (
                DefPath::from("support_crate::rvs_help_BI"),
                crate::capability::CapabilitySet::rvs_from_validated("BI"),
            ),
        ]);

        let local_scope = LocalScope::rvs_new(&BTreeSet::new());
        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &CapsMap::rvs_new(),
            &local_scope,
        );
        let output = format!("unknown={unknown:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_collect_std_unknown_callees_reports_non_emitted_support_crate",
            &output,
        );

        assert!(unknown.contains_key("support_crate::rvs_help_BI"));
    }

    #[test]
    fn test_20260706_collect_std_unknown_callees_skips_local_std_crate() {
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut local_std_node = crate::artifacts::FnNode::default();
        local_std_node
            .calls
            .insert(DefPath::from("dep::rvs_external_BI"));
        callgraph.rvs_insert_M(DefPath::from("std::rvs_local"), local_std_node);

        let inferred = BTreeMap::from([(
            DefPath::from("std::rvs_local"),
            crate::capability::CapabilitySet::rvs_new(),
        )]);
        let local_scope =
            LocalScope::rvs_new(&BTreeSet::from([crate::symbols::CrateName::from("std")]));

        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &CapsMap::rvs_new(),
            &local_scope,
        );
        let output = format!("unknown={unknown:?}\n");
        rvs_snapshot_BIS(
            "test_20260706_collect_std_unknown_callees_skips_local_std_crate",
            &output,
        );

        assert!(unknown.is_empty());
    }
}
