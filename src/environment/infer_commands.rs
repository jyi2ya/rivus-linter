use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::callgraph_cache::rvs_publish_std_callgraph_cache_BIST;
use super::cargo_targets::{CargoTargetScope, rvs_detect_local_crate_prefixes_BIS};
use super::workspace::{
    CallgraphCollectionMode, rvs_collect_callgraph_BIST, rvs_ensure_cargo_project_BIS,
    rvs_preflight_capsmap_file_BIS, rvs_validate_optional_capsmap_dir_BIS,
    rvs_write_pinned_capsmap_result_BIST,
};
use crate::callgraph::rvs_is_std_like_def_path;
use crate::capsmap::{CapsMap, rvs_reserved_layer_name};
use crate::function_classification::LocalScope;
#[cfg(test)]
use crate::inference::rvs_infer_caps_with_index;
use crate::inference::{
    CalleeCapsResolver, PreparedInference, rvs_build_impl_index, rvs_build_inference_dependents,
    rvs_format_def_path_capability_info, rvs_format_unknown_callees,
    rvs_generate_trait_alias_infos, rvs_incomplete_inference_paths_overlay_and_dependents,
    rvs_infer_caps_with_index_overlay_and_dependents, rvs_scope_port_methods_M,
};
#[cfg(test)]
use crate::symbols::CapsMapKey;
use crate::symbols::{DefPath, TraitMethodKey};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_infer_capsmap_BIPST(path: &Path, output: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let project_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;
    let _caps_lock = rvs_lock_caps_update_BIST(&project_path)?;
    let target_scope = CargoTargetScope::Production;
    let local_crate_names = rvs_detect_local_crate_prefixes_BIS(&project_path, target_scope)?;

    let abs_seed = project_path.join("caps");
    let caps_dir_exists = rvs_validate_optional_capsmap_dir_BIS(&abs_seed)?;
    let resolved_output = rvs_prepare_output_path_BIS(&project_path, output, "deps capsmap")?;
    let output_layer = rvs_caps_output_layer_BIS(&abs_seed, &resolved_output, caps_dir_exists)?;
    rvs_require_inference_output_layer(&output_layer.as_deref(), "deps", "infer-capsmap")?;
    let publication = rvs_pin_output_path_BIS(&resolved_output, "deps capsmap")?;
    let excluded_layers = [OsStr::new("deps")];
    let seed = CapsMap::rvs_load_effective_dir_excluding_names_BIS(&abs_seed, &excluded_layers)
        .map_err(|e| format!("caps: {e}"))?;

    let mut callgraph = rvs_collect_callgraph_BIST(
        &project_path,
        CallgraphCollectionMode::AllCrates,
        target_scope,
        &local_crate_names,
    )?;
    let inference = PreparedInference::rvs_prepare_M(&mut callgraph, &seed, &local_crate_names);
    let (direct_external_calls, unknown_callees) =
        inference.rvs_collect_direct_external_deps(&callgraph, &local_crate_names, &seed);

    if !unknown_callees.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown_callees,
            "error: the following external functions have no capability data.\n\
             Add them to caps/ext with the correct capability markers:\n\n",
        ));
    }

    let deps_result = rvs_format_def_path_capability_info(&direct_external_calls);
    rvs_write_pinned_capsmap_result_BIST(&deps_result, &publication, "deps capsmap")
}

pub(crate) fn rvs_run_infer_std_BIPST(path: &Path, output: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let project_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;
    let _caps_lock = rvs_lock_caps_update_BIST(&project_path)?;
    let target_scope = CargoTargetScope::Production;
    let local_crate_names = rvs_detect_local_crate_prefixes_BIS(&project_path, target_scope)?;
    let local_scope = LocalScope::rvs_new(&local_crate_names);

    let caps_dir = project_path.join("caps");
    let caps_dir_exists = rvs_validate_optional_capsmap_dir_BIS(&caps_dir)?;
    let resolved_output = rvs_prepare_output_path_BIS(&project_path, output, "std capsmap")?;
    let output_layer = rvs_caps_output_layer_BIS(&caps_dir, &resolved_output, caps_dir_exists)?;
    rvs_require_inference_output_layer(&output_layer.as_deref(), "std", "infer-std")?;
    let publication = rvs_pin_output_path_BIS(&resolved_output, "std capsmap")?;
    let seed = rvs_load_std_inference_seed_BIS(&caps_dir)?;
    let mut callgraph = rvs_collect_callgraph_BIST(
        &project_path,
        CallgraphCollectionMode::StandardLibrary,
        target_scope,
        &local_crate_names,
    )?;
    rvs_scope_port_methods_M(&mut callgraph, &local_crate_names);
    rvs_require_complete_std_collection(&callgraph, &local_scope)?;

    let pre_index = rvs_build_impl_index(&callgraph);
    let (mut inferred, incomplete_paths, post_alias_infos) =
        rvs_infer_std_with_trait_aliases(&callgraph, &seed, &pre_index);
    inferred.extend(
        post_alias_infos
            .iter()
            .map(|(path, info)| (path.clone(), info.rvs_caps().clone())),
    );

    let mut std_only: BTreeMap<DefPath, crate::capability::CapabilityInfo> = inferred
        .iter()
        .filter(|(name, _)| rvs_should_emit_std_capability(name, &local_scope))
        .map(|(path, caps)| {
            (
                path.clone(),
                rvs_std_inferred_capability_info(path, caps, &incomplete_paths),
            )
        })
        .collect();
    for (path, info) in post_alias_infos {
        if rvs_should_emit_std_capability(&path, &local_scope) {
            std_only.insert(path, info);
        }
    }

    let unknown =
        rvs_collect_std_unknown_callees(&callgraph, &inferred, &seed, &pre_index, &local_scope);

    if !unknown.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown,
            "error: the following functions are called by std but have no capability data.\n\
             Update the distributed seed for this toolchain, or add an explicit project caps/seed override:\n\n",
        ));
    }

    let result = rvs_format_def_path_capability_info(&std_only);
    rvs_write_pinned_capsmap_result_BIST(&result, &publication, "std capsmap")?;
    rvs_publish_std_callgraph_cache_BIST(&project_path, &callgraph)
        .map_err(|error| error.to_string())
}

fn rvs_load_std_inference_seed_BIS(caps_dir: &Path) -> Result<CapsMap, String> {
    CapsMap::rvs_load_effective_dir_layers_BIS(caps_dir, &["seed", "suppress"])
        .map_err(|e| format!("caps: {e}"))
}

fn rvs_lock_caps_update_BIST(
    project_path: &Path,
) -> Result<super::fs_guard::RivusDirectoryLock, String> {
    super::fs_guard::rvs_try_lock_directory_BIST(project_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            format!(
                "another caps inference command is already running for {}",
                project_path.display()
            )
        } else {
            format!(
                "cannot lock caps update directory {}: {error}",
                project_path.display()
            )
        }
    })
}

fn rvs_infer_std_with_trait_aliases(
    callgraph: &crate::artifacts::FnGraph,
    seed: &CapsMap,
    impl_index: &std::collections::HashMap<crate::symbols::TraitMethodKey, Vec<DefPath>>,
) -> (
    BTreeMap<DefPath, crate::capability::CapabilitySet>,
    BTreeSet<DefPath>,
    BTreeMap<DefPath, crate::capability::CapabilityInfo>,
) {
    let dependents = rvs_build_inference_dependents(callgraph, impl_index);
    let empty_overlay = BTreeMap::new();
    let inferred = rvs_infer_caps_with_index_overlay_and_dependents(
        callgraph,
        seed,
        &empty_overlay,
        impl_index,
        &dependents,
    );
    let incomplete = rvs_incomplete_inference_paths_overlay_and_dependents(
        callgraph,
        seed,
        &empty_overlay,
        &inferred,
        impl_index,
        &dependents,
    );
    let aliases = rvs_generate_trait_alias_infos(&inferred, impl_index, callgraph, &incomplete)
        .into_iter()
        .filter(|(path, _)| seed.rvs_lookup_def_path(path).is_none())
        .collect();
    (inferred, incomplete, aliases)
}

fn rvs_std_inferred_capability_info(
    path: &DefPath,
    caps: &crate::capability::CapabilitySet,
    incomplete_paths: &BTreeSet<DefPath>,
) -> crate::capability::CapabilityInfo {
    let completeness = if incomplete_paths.contains(path) {
        crate::capability::CapabilityCompleteness::Incomplete
    } else {
        crate::capability::CapabilityCompleteness::Complete
    };
    crate::capability::CapabilityInfo::rvs_new(
        caps.clone(),
        crate::capability::CapabilityBasis::Inferred,
        completeness,
    )
}

fn rvs_should_emit_std_capability(path: &DefPath, local_scope: &LocalScope) -> bool {
    !local_scope.rvs_contains(path) && rvs_is_std_like_def_path(path.rvs_as_str())
}

fn rvs_require_inference_output_layer(
    output_layer: &Option<&OsStr>,
    expected_layer: &str,
    command: &str,
) -> Result<(), String> {
    let Some(output_layer) = output_layer else {
        return Ok(());
    };
    let output_name = output_layer.to_string_lossy();
    if let Some(reserved_layer) = rvs_reserved_layer_name(output_layer) {
        if reserved_layer != expected_layer {
            return Err(format!(
                "{command} output cannot replace reserved caps layer '{}'; expected '{expected_layer}'",
                output_layer.to_string_lossy()
            ));
        }
        if *output_layer != OsStr::new(expected_layer) {
            return Err(format!(
                "{command} output layer '{}' must use canonical lowercase name '{expected_layer}'",
                output_layer.to_string_lossy()
            ));
        }
    }
    if *output_layer != OsStr::new(expected_layer) {
        return Err(format!(
            "{command} output inside the active caps directory must use canonical layer '{expected_layer}', not '{output_name}'; custom output paths must be outside caps/"
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
        let user_path = name.rvs_user_path();
        has_std |= user_path.starts_with("std::");
        has_core |= user_path.starts_with("core::");
        has_alloc |= user_path.starts_with("alloc::");
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
    rvs_assert_output_parent_BIS(&resolved, label)?;
    Ok(resolved)
}

fn rvs_assert_output_parent_BIS(path: &Path, label: &str) -> Result<(), String> {
    let Some(output_parent) = path.parent() else {
        return Ok(());
    };
    let mut ancestor = output_parent;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "{label} output parent '{}' must not contain symlinked or non-directory components",
                    output_parent.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect {label} output parent '{}': {error}",
                    output_parent.display()
                ));
            }
        }
        let Some(parent) = ancestor.parent() else {
            break;
        };
        if parent == ancestor {
            break;
        }
        ancestor = parent;
    }
    Ok(())
}

fn rvs_pin_output_path_BIS(path: &Path, label: &str) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create {label} output parent '{}': {error}",
                parent.display()
            )
        })?;
    }
    Ok(path.to_path_buf())
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
    let alias_roots_match = rvs_caps_output_roots_match_BIS(parent, caps_dir)?;
    let aliases_caps_name = parent != caps_dir
        && parent
            .file_name()
            .and_then(OsStr::to_str)
            .zip(caps_dir.file_name().and_then(OsStr::to_str))
            .is_some_and(|(parent_name, caps_name)| parent_name.eq_ignore_ascii_case(caps_name))
        && alias_roots_match;
    if aliases_caps_name {
        return Err(format!(
            "capsmap output parent '{}' aliases canonical caps directory '{}'; use the lowercase path",
            parent.display(),
            caps_dir.display()
        ));
    }
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
            let roots_match = rvs_caps_output_roots_match_BIS(parent, caps_dir)?;
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
    if parent != caps_dir {
        return Err(format!(
            "capsmap output parent '{}' aliases canonical caps directory '{}'; use the exact lexical caps path",
            parent.display(),
            caps_dir.display()
        ));
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

fn rvs_caps_output_roots_match_BIS(parent: &Path, caps_dir: &Path) -> Result<bool, String> {
    match (parent.parent(), caps_dir.parent()) {
        (Some(parent_root), Some(caps_root)) if parent_root == caps_root => Ok(true),
        (Some(parent_root), Some(caps_root)) if parent_root.is_dir() && caps_root.is_dir() => {
            same_file::is_same_file(parent_root, caps_root).map_err(|error| {
                format!(
                    "cannot compare capsmap output root '{}' with project root '{}': {error}",
                    parent_root.display(),
                    caps_root.display()
                )
            })
        }
        _ => Ok(false),
    }
}

fn rvs_collect_std_unknown_callees(
    callgraph: &crate::artifacts::FnGraph,
    inferred: &BTreeMap<DefPath, crate::capability::CapabilitySet>,
    seed: &CapsMap,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    local_scope: &LocalScope,
) -> BTreeMap<DefPath, BTreeSet<DefPath>> {
    let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
    let resolver = CalleeCapsResolver::rvs_new(callgraph, seed, inferred, impl_index);
    for (func, behavior) in callgraph.rvs_iter() {
        let is_std = rvs_is_std_like_def_path(func.rvs_as_str());
        let is_local = local_scope.rvs_contains(func);
        if !is_std || is_local {
            continue;
        }
        for callee in behavior.rvs_dependency_calls() {
            if rvs_is_indirect_uncertainty(callee) {
                continue;
            }
            if resolver.rvs_for_contract_check(callee).is_some() {
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

fn rvs_is_indirect_uncertainty(callee: &DefPath) -> bool {
    callee
        .rvs_as_str()
        .ends_with("::{unknown_indirect_fn_pointer}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallEdgeType, FunctionIdentity};
    use crate::test_support::{
        rvs_caps_v2, rvs_make_capsmap, rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS,
        rvs_snapshot_BIS,
    };

    #[test]
    fn test_20260717_infer_std_uses_distributed_seed_without_project_seed() {
        let dir = rvs_make_temp_dir_BIS("infer-std-distributed-seed");
        let caps_dir = dir.join("caps");
        let seed = rvs_load_std_inference_seed_BIS(&caps_dir).unwrap();
        let info = seed
            .rvs_lookup_info("alloc::alloc::__rust_alloc")
            .expect("never: the distributed seed contains the allocator boundary");
        let output = format!(
            "project_caps_exists={}\ncaps={}\nsource={}\n",
            caps_dir.exists(),
            info.rvs_caps().rvs_letters(),
            info.rvs_source()
                .expect("never: distributed records retain their source")
                .file
                .display(),
        );
        rvs_snapshot_BIS(
            "test_20260717_infer_std_uses_distributed_seed_without_project_seed",
            &output,
        );

        assert!(!caps_dir.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260731_distributed_seed_covers_posix_spawn_boundary() {
        let dir = rvs_make_temp_dir_BIS("infer-std-posix-spawn-seed");
        let caps_dir = dir.join("caps");
        let seed = rvs_load_std_inference_seed_BIS(&caps_dir).unwrap();
        let info = seed
            .rvs_lookup_info("libc::unix::linux_like::linux::posix_spawnp")
            .expect("never: the distributed seed contains the process-spawn boundary");
        let output = format!(
            "project_caps_exists={}\ncaps={}\nsource={}\n",
            caps_dir.exists(),
            info.rvs_caps().rvs_letters(),
            info.rvs_source()
                .expect("never: distributed records retain their source")
                .file
                .display(),
        );
        rvs_snapshot_BIS(
            "test_20260731_distributed_seed_covers_posix_spawn_boundary",
            &output,
        );

        assert_eq!(info.rvs_caps().rvs_letters(), "BIS");
        assert!(!caps_dir.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260801_distributed_seed_covers_supported_posix_spawn_families() {
        let dir = rvs_make_temp_dir_BIS("infer-std-posix-spawn-families");
        let seed = rvs_load_std_inference_seed_BIS(&dir.join("caps")).unwrap();
        let families = [
            ("linux-android", "libc::unix::linux_like::linux"),
            ("apple-source-declaration", "libc::unix::bsd::apple"),
        ];
        let expected = [
            ("posix_spawn", "BIS"),
            ("posix_spawn_file_actions_adddup2", ""),
            ("posix_spawn_file_actions_destroy", "U"),
            ("posix_spawn_file_actions_init", "U"),
            ("posix_spawnattr_destroy", "U"),
            ("posix_spawnattr_init", "U"),
            ("posix_spawnattr_setflags", ""),
            ("posix_spawnattr_setpgroup", ""),
            ("posix_spawnattr_setsigdefault", ""),
            ("posix_spawnp", "BIS"),
        ];
        let mut output = String::new();
        for (family, prefix) in families {
            let matched = expected
                .iter()
                .filter(|(name, caps)| {
                    seed.rvs_lookup_info(&format!("{prefix}::{name}"))
                        .is_some_and(|info| info.rvs_caps().rvs_letters() == *caps)
                })
                .count();
            output.push_str(&format!(
                "{family}: exact_source_records={matched}/{} spawn_caps={} spawnp_caps={}\n",
                expected.len(),
                seed.rvs_lookup_info(&format!("{prefix}::posix_spawn"))
                    .map(|info| info.rvs_caps().rvs_letters())
                    .unwrap_or_else(|| "missing".to_string()),
                seed.rvs_lookup_info(&format!("{prefix}::posix_spawnp"))
                    .map(|info| info.rvs_caps().rvs_letters())
                    .unwrap_or_else(|| "missing".to_string()),
            ));
        }
        output.push_str("apple_runtime_executed=false\n");
        rvs_snapshot_BIS(
            "test_20260801_distributed_seed_covers_supported_posix_spawn_families",
            &output,
        );

        assert!(output.lines().take(2).all(|line| line.contains("10/10")));
        assert!(
            output
                .lines()
                .take(2)
                .all(|line| line.matches("BIS").count() == 2)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260716_infer_std_renders_complete_and_incomplete_inferred_records() {
        let inferred = BTreeMap::from([
            (
                DefPath::from("core::complete"),
                crate::capability::CapabilitySet::rvs_from_validated("B"),
            ),
            (
                DefPath::from("std::incomplete"),
                crate::capability::CapabilitySet::rvs_from_validated("IS"),
            ),
        ]);
        let incomplete_paths = BTreeSet::from([DefPath::from("std::incomplete")]);
        let infos = inferred
            .iter()
            .map(|(path, caps)| {
                (
                    path.clone(),
                    rvs_std_inferred_capability_info(path, caps, &incomplete_paths),
                )
            })
            .collect();
        let rendered = rvs_format_def_path_capability_info(&infos);
        rvs_snapshot_BIS(
            "test_20260716_infer_std_renders_complete_and_incomplete_inferred_records",
            &rendered,
        );

        let parsed = CapsMap::rvs_parse(&rendered).unwrap();
        assert_eq!(
            parsed
                .rvs_lookup_info("core::complete")
                .unwrap()
                .rvs_completeness(),
            crate::capability::CapabilityCompleteness::Complete
        );
        assert_eq!(
            parsed
                .rvs_lookup_info("std::incomplete")
                .unwrap()
                .rvs_completeness(),
            crate::capability::CapabilityCompleteness::Incomplete
        );
    }

    #[test]
    fn test_20260716_infer_std_trait_alias_incompleteness_reaches_callers() {
        let trait_path = DefPath::from("std::Parser::rvs_parse");
        let impl_path = DefPath::from("std::Adapter::rvs_parse@std::Parser");
        let caller_path = DefPath::from("std::rvs_use_parser");
        let mut graph = crate::artifacts::FnGraph::rvs_new();
        graph.rvs_insert_M(
            trait_path.clone(),
            crate::artifacts::FnNode {
                has_body: false,
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            impl_path,
            crate::artifacts::FnNode {
                calls: BTreeMap::from([(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from("dependency::incomplete"),
                    },
                    CallEdgeType::Strong,
                )]),
                is_trait_impl: true,
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            caller_path.clone(),
            crate::artifacts::FnNode {
                calls: BTreeMap::from([(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: trait_path.clone(),
                    },
                    CallEdgeType::Strong,
                )]),
                ..crate::artifacts::FnNode::default()
            },
        );
        let mut seed = CapsMap::rvs_new();
        seed.rvs_insert_info_M(
            CapsMapKey::from("dependency::incomplete"),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_from_validated("S"),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Unknown,
            ),
        );
        let impl_index = rvs_build_impl_index(&graph);

        let (_, incomplete, aliases) = rvs_infer_std_with_trait_aliases(&graph, &seed, &impl_index);
        let output = format!(
            "trait_incomplete={}\ncaller_incomplete={}\nalias_completeness={}\n",
            incomplete.contains(&trait_path),
            incomplete.contains(&caller_path),
            aliases
                .get(&trait_path)
                .map(|info| info.rvs_completeness().rvs_name())
                .unwrap_or("missing"),
        );
        rvs_snapshot_BIS(
            "test_20260716_infer_std_trait_alias_incompleteness_reaches_callers",
            &output,
        );

        assert!(incomplete.contains(&caller_path));
    }

    #[test]
    fn test_20260716_infer_std_seed_precedes_generated_trait_alias() {
        let trait_path = DefPath::from("std::Parser::rvs_parse");
        let impl_path = DefPath::from("std::Adapter::rvs_parse@std::Parser");
        let caller_path = DefPath::from("std::rvs_use_parser");
        let mut graph = crate::artifacts::FnGraph::rvs_new();
        graph.rvs_insert_M(
            trait_path.clone(),
            crate::artifacts::FnNode {
                has_body: false,
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            impl_path.clone(),
            crate::artifacts::FnNode {
                is_trait_impl: true,
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            caller_path.clone(),
            crate::artifacts::FnNode {
                calls: BTreeMap::from([(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: trait_path.clone(),
                    },
                    CallEdgeType::Strong,
                )]),
                ..crate::artifacts::FnNode::default()
            },
        );
        let mut seed = CapsMap::rvs_new();
        seed.rvs_insert_info_M(
            CapsMapKey::from(trait_path.clone()),
            crate::capability::CapabilityInfo::rvs_explicit(
                crate::capability::CapabilitySet::rvs_from_validated("B"),
            ),
        );
        seed.rvs_insert_info_M(
            CapsMapKey::from(impl_path),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_from_validated("S"),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Unknown,
            ),
        );
        let impl_index = rvs_build_impl_index(&graph);

        let (inferred, incomplete, aliases) =
            rvs_infer_std_with_trait_aliases(&graph, &seed, &impl_index);
        let output = format!(
            "caller_caps={}\ncaller_incomplete={}\ngenerated_alias={}\n",
            inferred
                .get(&caller_path)
                .map_or("missing".to_string(), ToString::to_string),
            incomplete.contains(&caller_path),
            aliases.contains_key(&trait_path),
        );
        rvs_snapshot_BIS(
            "test_20260716_infer_std_seed_precedes_generated_trait_alias",
            &output,
        );

        assert_eq!(
            inferred.get(&caller_path),
            Some(&crate::capability::CapabilitySet::rvs_from_validated("B"))
        );
        assert!(!incomplete.contains(&caller_path));
        assert!(!aliases.contains_key(&trait_path));
    }

    #[test]
    fn test_20260729_infer_std_support_impl_vote_reaches_std_caller() {
        let trait_path = DefPath::from("std::Parser::rvs_parse");
        let implementation_path = DefPath::from("support_crate::ParserImpl::rvs_parse@std::Parser");
        let caller_path = DefPath::from("std::rvs_use_parser");
        let mut graph = crate::artifacts::FnGraph::rvs_new();
        graph.rvs_insert_M(trait_path.clone(), crate::artifacts::FnNode::default());
        graph.rvs_insert_M(
            implementation_path.clone(),
            crate::artifacts::FnNode {
                facts: crate::capability::CapabilityFacts {
                    has_static_ref: true,
                    ..crate::capability::CapabilityFacts::default()
                },
                is_trait_impl: true,
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            caller_path.clone(),
            crate::artifacts::FnNode {
                calls: BTreeMap::from([(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: trait_path.clone(),
                    },
                    CallEdgeType::Strong,
                )]),
                ..crate::artifacts::FnNode::default()
            },
        );
        let seed = CapsMap::rvs_new();
        let impl_index = rvs_build_impl_index(&graph);

        let (mut inferred, incomplete, aliases) =
            rvs_infer_std_with_trait_aliases(&graph, &seed, &impl_index);
        let caller_caps = inferred
            .get(&caller_path)
            .map(crate::capability::CapabilitySet::rvs_letters)
            .unwrap_or_else(|| "missing".to_string());
        let alias_caps = aliases
            .get(&trait_path)
            .map(|info| info.rvs_caps().rvs_letters())
            .unwrap_or_else(|| "missing".to_string());
        let support_inferred = inferred.contains_key(&implementation_path);
        inferred.extend(
            aliases
                .iter()
                .map(|(path, info)| (path.clone(), info.rvs_caps().clone())),
        );
        let local_scope = LocalScope::rvs_new(&BTreeSet::new());
        let mut std_only: BTreeMap<DefPath, crate::capability::CapabilityInfo> = inferred
            .iter()
            .filter(|(path, _)| rvs_should_emit_std_capability(path, &local_scope))
            .map(|(path, caps)| {
                (
                    path.clone(),
                    rvs_std_inferred_capability_info(path, caps, &incomplete),
                )
            })
            .collect();
        for (path, info) in &aliases {
            if rvs_should_emit_std_capability(path, &local_scope) {
                std_only.insert(path.clone(), info.clone());
            }
        }
        let support_serialized = std_only.contains_key(&implementation_path);
        let serialized_paths = std_only
            .keys()
            .map(DefPath::rvs_as_str)
            .collect::<Vec<_>>()
            .join(",");
        let output = format!(
            "caller_caps={caller_caps}\nalias_caps={alias_caps}\nsupport_inferred={support_inferred}\nsupport_serialized={support_serialized}\nserialized_paths={serialized_paths}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_infer_std_support_impl_vote_reaches_std_caller",
            &output,
        );

        assert_eq!(caller_caps, "S");
        assert_eq!(alias_caps, "S");
        assert!(support_inferred);
        assert!(!support_serialized);
        assert_eq!(
            serialized_paths,
            "std::Parser::rvs_parse,std::rvs_use_parser"
        );
    }

    #[test]
    fn test_20260730_infer_std_preserves_known_io_lower_bound_through_incomplete_chain() {
        let read_path = DefPath::from("std::fs::read_to_string");
        let metadata_path = DefPath::from("std::fs::File::metadata");
        let stat_path = DefPath::from("libc::unix::linux_like::fstat64");
        let opaque_path = DefPath::from("support_crate::opaque_branch");
        let mut graph = crate::artifacts::FnGraph::rvs_new();
        graph.rvs_insert_M(
            read_path.clone(),
            crate::artifacts::FnNode {
                calls: BTreeMap::from([
                    (
                        FunctionIdentity {
                            crate_id: 1,
                            def_path: metadata_path.clone(),
                        },
                        CallEdgeType::Strong,
                    ),
                    (
                        FunctionIdentity {
                            crate_id: 1,
                            def_path: opaque_path.clone(),
                        },
                        CallEdgeType::Strong,
                    ),
                ]),
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            metadata_path.clone(),
            crate::artifacts::FnNode {
                calls: BTreeMap::from([(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: stat_path,
                    },
                    CallEdgeType::Strong,
                )]),
                ..crate::artifacts::FnNode::default()
            },
        );
        graph.rvs_insert_M(
            opaque_path,
            crate::artifacts::FnNode {
                has_body: false,
                complete: false,
                ..crate::artifacts::FnNode::default()
            },
        );
        let seed = crate::capsmap::rvs_load_distributed_seed().unwrap();
        let impl_index = rvs_build_impl_index(&graph);

        let (inferred, incomplete, _) =
            rvs_infer_std_with_trait_aliases(&graph, &seed, &impl_index);
        let read_caps = inferred
            .get(&read_path)
            .map(crate::capability::CapabilitySet::rvs_letters)
            .unwrap_or_else(|| "missing".to_string());
        let metadata_caps = inferred
            .get(&metadata_path)
            .map(crate::capability::CapabilitySet::rvs_letters)
            .unwrap_or_else(|| "missing".to_string());
        let read_incomplete = incomplete.contains(&read_path);
        let output = format!(
            "read_caps={read_caps}\nmetadata_caps={metadata_caps}\nread_incomplete={read_incomplete}\n"
        );
        rvs_snapshot_BIS(
            "test_20260730_infer_std_preserves_known_io_lower_bound_through_incomplete_chain",
            &output,
        );

        assert_eq!(read_caps, "BI");
        assert_eq!(metadata_caps, "BI");
        assert!(read_incomplete);
    }

    #[test]
    fn test_20260716_inference_rejects_temporary_shaped_output_layer() {
        let cases = [
            ".deps.123.0.tmp",
            ".generated.123.0.tmp",
            ".custom.123.0.tmp",
        ];
        let output = cases
            .into_iter()
            .map(|layer| {
                format!(
                    "{layer}: {:?}",
                    rvs_require_inference_output_layer(
                        &Some(OsStr::new(layer)),
                        "deps",
                        "infer-capsmap",
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS(
            "test_20260716_inference_rejects_temporary_shaped_output_layer",
            &output,
        );
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_inference_rejects_concurrent_caps_update() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-concurrent-caps-update",
            "infer-concurrent-caps-update",
            &[("src/lib.rs", "pub fn rvs_value() -> u8 { 1 }\n")],
        );
        let lock = crate::environment::fs_guard::rvs_try_lock_directory_BIST(&dir).unwrap();

        let error = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps")).unwrap_err();
        let output = format!("{error}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_inference_rejects_concurrent_caps_update",
            &output,
        );

        assert!(error.contains("already running"));
        drop(lock);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_infer_capsmap_rejects_workspace_root() {
        let dir = rvs_make_temp_dir_BIS("infer-capsmap-workspace-root");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps"));
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

        let result = rvs_run_infer_std_BIPST(&dir, Path::new("caps/std"));
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

        let result = rvs_run_infer_std_BIPST(&dir, Path::new("caps/std"));
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260706_infer_std_rejects_caps_file", &output);

        assert!(result.is_err());
        assert!(output.contains("is not a directory"));

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

        let result = rvs_run_infer_std_BIPST(&dir, Path::new("caps/std"));
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_infer_std_rejects_broken_caps_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("symlink") || output.contains("not a directory"));

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
        std::fs::write(
            dir.join("caps/seed"),
            "# rivus-caps-v2\n{\"path\":\"demo\",\"caps\":\"Z\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
        )
        .unwrap();

        let result = rvs_run_infer_std_BIPST(&dir, Path::new("caps/std"));
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

        let result = rvs_run_infer_std_BIPST(&dir, &output_path);
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

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/seed"));
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

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("CAPS/seed"));
        let callgraph_exists = dir.join("target/rivus-callgraph").exists();
        let output_exists = dir.join("CAPS/seed").exists();
        let output = format!(
            "result={result:?}\ncallgraph_exists={callgraph_exists}\noutput_exists={output_exists}\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_infer_capsmap_rejects_case_aliased_missing_caps_seed",
            &output,
        );

        assert!(result.is_err());
        assert!(!callgraph_exists);
        assert!(!output_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_infer_capsmap_rejects_case_aliased_existing_caps_output() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-case-aliased-existing-caps",
            "infer-capsmap-case-aliased-existing-caps",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir(dir.join("caps")).unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("CAPS/generated"));
        let output = format!(
            "result={result:?}\nshadow_exists={}\n",
            dir.join("CAPS").exists()
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_infer_capsmap_rejects_case_aliased_existing_caps_output",
            &output,
        );

        assert!(result.is_err());
        assert!(!dir.join("CAPS").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_infer_capsmap_rejects_case_alias_through_project_symlink() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-case-alias-project-symlink",
            "infer-capsmap-case-alias-project-symlink",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir(dir.join("caps")).unwrap();
        let link = dir.with_file_name(format!(
            "{}-link",
            dir.file_name()
                .expect("never: temp project has a file name")
                .to_string_lossy()
        ));
        std::os::unix::fs::symlink(&dir, &link).unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, &link.join("CAPS/generated"));
        let output = format!(
            "result={result:?}\nshadow_exists={}\n",
            link.join("CAPS").exists()
        )
        .replace(&link.to_string_lossy().into_owned(), "$LINK")
        .replace(&dir.to_string_lossy().into_owned(), "$PROJECT");
        rvs_snapshot_BIS(
            "test_20260715_infer_capsmap_rejects_case_alias_through_project_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(!link.join("CAPS").exists());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_caps_output_layer_rejects_multiple_hardlink_names() {
        let dir = rvs_make_temp_dir_BIS("caps-output-hardlink-alias");
        let caps_dir = dir.join("caps");
        std::fs::create_dir_all(&caps_dir).unwrap();
        std::fs::write(caps_dir.join("seed"), rvs_caps_v2(&[("manual", "B")])).unwrap();
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
        std::fs::write(
            dir.join("caps/seed"),
            "# rivus-caps-v2\n{\"path\":\"broken\",\"caps\":\"Z\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
        )
        .unwrap();
        let sentinel = "dependency::entry=I\n";
        std::fs::write(dir.join("caps/deps"), sentinel).unwrap();

        let result = rvs_run_infer_std_BIPST(&dir, Path::new("caps/deps"));
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
            ("infer-capsmap", "deps", Some("DEPS"), true),
            ("infer-capsmap", "deps", Some("generated"), true),
            ("infer-capsmap", "deps", Some("std"), true),
            ("infer-capsmap", "deps", Some("seed"), true),
            ("infer-capsmap", "deps", Some("SEED"), true),
            ("infer-capsmap", "deps", Some("suppress"), true),
            ("infer-capsmap", "deps", Some("ext"), true),
            ("infer-std", "std", None, false),
            ("infer-std", "std", Some("std"), false),
            ("infer-std", "std", Some("STD"), true),
            ("infer-std", "std", Some("generated"), true),
            ("infer-std", "std", Some("deps"), true),
            ("infer-std", "std", Some("DEPS"), true),
            ("infer-std", "std", Some("seed"), true),
            ("infer-std", "std", Some("suppress"), true),
            ("infer-std", "std", Some("ext"), true),
        ];
        let mut output = String::new();
        for (command, expected, layer, should_error) in cases {
            let result =
                rvs_require_inference_output_layer(&layer.map(OsStr::new), expected, command);
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
    fn test_20260729_infer_capsmap_rejects_inside_caps_and_accepts_outside() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-canonical-active-layer",
            "infer-capsmap-canonical-active-layer",
            &[("src/lib.rs", "pub fn rvs_value() -> u8 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/ext"),
            rvs_caps_v2(&[("manual::correction", "S")]),
        )
        .unwrap();
        let sentinel = rvs_caps_v2(&[("manual::correction", "B")]);
        std::fs::write(dir.join("caps/generated"), &sentinel).unwrap();

        let inside = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/generated"));
        let inside_preserved =
            std::fs::read_to_string(dir.join("caps/generated")).unwrap() == sentinel;
        let outside = rvs_run_infer_capsmap_BIPST(&dir, Path::new("generated-caps"));
        let outside_header = std::fs::read_to_string(dir.join("generated-caps"))
            .unwrap()
            .starts_with("# rivus-caps-v2\n");
        let output = format!(
            "inside_error={}\ninside_preserved={inside_preserved}\noutside_ok={}\noutside_header={outside_header}\n",
            inside.is_err(),
            outside.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260729_infer_capsmap_rejects_inside_caps_and_accepts_outside",
            &output,
        );

        assert!(inside.is_err());
        assert!(inside_preserved);
        assert!(outside.is_ok());
        assert!(outside_header);
        std::fs::remove_dir_all(dir).unwrap();
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

        let result = rvs_run_infer_capsmap_BIPST(&dir, &output_path);
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

        let result = rvs_run_infer_capsmap_BIPST(&dir, &output_path);
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
        std::fs::write(
            dir.join("caps/seed"),
            "# rivus-caps-v2\n{\"path\":\"demo\",\"caps\":\"Z\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
        )
        .unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps"));
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
        std::fs::write(
            dir.join("caps/deps"),
            "# rivus-caps-v2\n{\"path\":\"broken\",\"caps\":\"Z\",\"basis\":{\"kind\":\"inferred\"},\"completeness\":\"complete\"}\n",
        )
        .unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps"));
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
        std::fs::write(dir.join("generated"), "broken=Z\n").unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("generated"));
        assert!(
            result.is_ok(),
            "custom output outside active caps should remain allowed: {result:?}"
        );
        let generated = std::fs::read_to_string(dir.join("generated")).unwrap();
        let output = format!("result={result:?}\ngenerated={generated:?}\n");
        rvs_snapshot_BIS(
            "test_20260714_infer_capsmap_replaces_invalid_custom_output",
            &output,
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260729_infer_capsmap_rejects_active_caps_output_through_symlink() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-invalid-symlink-output",
            "infer-capsmap-invalid-symlink-output",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("caps/generated"), "broken=Z\n").unwrap();
        std::os::unix::fs::symlink(dir.join("caps"), dir.join("caps-alias")).unwrap();

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps-alias/generated"));
        let generated_preserved =
            std::fs::read_to_string(dir.join("caps/generated")).unwrap() == "broken=Z\n";
        let output = format!(
            "is_err={}\ngenerated_preserved={generated_preserved}\n",
            result.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260729_infer_capsmap_rejects_active_caps_output_through_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(generated_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260731_inference_rejects_deps_and_std_through_caps_symlink_alias() {
        let dir = rvs_make_temp_dir_BIS("inference-canonical-caps-parent");
        let caps_dir = dir.join("caps");
        std::fs::create_dir(&caps_dir).unwrap();
        let deps_sentinel = rvs_caps_v2(&[("dependency::sentinel", "I")]);
        let std_sentinel = rvs_caps_v2(&[("std::sentinel", "B")]);
        std::fs::write(caps_dir.join("deps"), &deps_sentinel).unwrap();
        std::fs::write(caps_dir.join("std"), &std_sentinel).unwrap();
        std::os::unix::fs::symlink(&caps_dir, dir.join("caps-alias")).unwrap();

        let deps_output = dir.join("caps-alias/deps");
        let deps_result = rvs_prepare_output_path_BIS(&dir, &deps_output, "deps capsmap")
            .and_then(|prepared| rvs_caps_output_layer_BIS(&caps_dir, &prepared, true))
            .and_then(|layer| {
                rvs_require_inference_output_layer(&layer.as_deref(), "deps", "infer-capsmap")
            });
        let std_output = dir.join("caps-alias/std");
        let std_result = rvs_prepare_output_path_BIS(&dir, &std_output, "std capsmap")
            .and_then(|prepared| rvs_caps_output_layer_BIS(&caps_dir, &prepared, true))
            .and_then(|layer| {
                rvs_require_inference_output_layer(&layer.as_deref(), "std", "infer-std")
            });
        let deps_preserved =
            std::fs::read_to_string(caps_dir.join("deps")).is_ok_and(|text| text == deps_sentinel);
        let std_preserved =
            std::fs::read_to_string(caps_dir.join("std")).is_ok_and(|text| text == std_sentinel);
        let output = format!(
            "deps_rejected={}\nstd_rejected={}\ndeps_preserved={deps_preserved}\nstd_preserved={std_preserved}\n",
            deps_result.is_err(),
            std_result.is_err(),
        );
        rvs_snapshot_BIS(
            "test_20260731_inference_rejects_deps_and_std_through_caps_symlink_alias",
            &output,
        );

        assert!(deps_result.is_err());
        assert!(std_result.is_err());
        assert!(deps_preserved);
        assert!(std_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260729_infer_capsmap_rejects_non_utf8_active_caps_output() {
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

        let result = rvs_run_infer_capsmap_BIPST(&dir, &output_path);
        let generated_preserved =
            std::fs::read_to_string(dir.join(&output_path)).unwrap() == "broken=Z\n";
        let output = format!(
            "is_err={}\ngenerated_preserved={generated_preserved}\n",
            result.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260729_infer_capsmap_rejects_non_utf8_active_caps_output",
            &output,
        );

        assert!(result.is_err());
        assert!(generated_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_infer_capsmap_creates_missing_caps_dir() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-missing-caps-dir",
            "infer-capsmap-missing-caps-dir",
            &[("src/lib.rs", "pub fn rvs_add() -> i32 { 1 }\n")],
        );

        let result = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps"));
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
    fn test_20260717_collect_std_unknown_callees_accepts_inferred_support_crate() {
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut std_node = crate::artifacts::FnNode::default();
        std_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("support_crate::help"),
            },
            CallEdgeType::Strong,
        );
        callgraph.rvs_insert_M(DefPath::from("std::fs::read_to_string"), std_node);
        let mut support_node = crate::artifacts::FnNode::default();
        support_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("ffi_support::rvs_read_BI"),
            },
            CallEdgeType::Strong,
        );
        callgraph.rvs_insert_M(DefPath::from("support_crate::help"), support_node);

        let local_scope = LocalScope::rvs_new(&BTreeSet::new());
        let impl_index = rvs_build_impl_index(&callgraph);
        // The ffi boundary is known through the capsmap, not through its
        // name: suffixes are views over semantic caps, never sources.
        let seed = rvs_make_capsmap(&[("ffi_support::rvs_read_BI", "BI")]);
        let inferred = rvs_infer_caps_with_index(&callgraph, &seed, &impl_index);
        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &seed,
            &impl_index,
            &local_scope,
        );
        let support_caps = inferred
            .get("support_crate::help")
            .map(crate::capability::CapabilitySet::rvs_letters)
            .unwrap_or_else(|| "missing".into());
        let std_emitted =
            rvs_should_emit_std_capability(&DefPath::from("std::fs::read_to_string"), &local_scope);
        let support_emitted =
            rvs_should_emit_std_capability(&DefPath::from("support_crate::help"), &local_scope);
        let output = format!(
            "support_caps={support_caps}\nstd_emitted={std_emitted}\nsupport_emitted={support_emitted}\nunknown={unknown:?}\n"
        );
        rvs_snapshot_BIS(
            "test_20260717_collect_std_unknown_callees_accepts_inferred_support_crate",
            &output,
        );

        assert_eq!(support_caps, "BI");
        assert!(std_emitted);
        assert!(!support_emitted);
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260717_collect_std_unknown_callees_reports_bodyless_support_boundary() {
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut std_node = crate::artifacts::FnNode::default();
        std_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("support_crate::opaque_boundary"),
            },
            CallEdgeType::Strong,
        );
        callgraph.rvs_insert_M(DefPath::from("std::fs::read_to_string"), std_node);
        callgraph.rvs_insert_M(
            DefPath::from("support_crate::opaque_boundary"),
            crate::artifacts::FnNode {
                has_body: false,
                ..crate::artifacts::FnNode::default()
            },
        );
        let local_scope = LocalScope::rvs_new(&BTreeSet::new());
        let impl_index = rvs_build_impl_index(&callgraph);
        let inferred = rvs_infer_caps_with_index(&callgraph, &CapsMap::rvs_new(), &impl_index);
        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &CapsMap::rvs_new(),
            &impl_index,
            &local_scope,
        );
        let output = format!("unknown={unknown:?}\n");
        rvs_snapshot_BIS(
            "test_20260717_collect_std_unknown_callees_reports_bodyless_support_boundary",
            &output,
        );

        assert!(unknown.contains_key("support_crate::opaque_boundary"));
    }

    #[test]
    fn test_20260731_collect_std_unknown_callees_preserves_generic_uncertainty() {
        let caller = DefPath::from("std::option::Option::map");
        let synthetic =
            DefPath::from("std::option::Option::map::closure#0::{unknown_indirect_fn_pointer}");
        let external = DefPath::from("ffi_support::opaque_boundary");
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut std_node = crate::artifacts::FnNode::default();
        std_node.calls = BTreeMap::from([
            (
                crate::artifacts::FunctionIdentity {
                    crate_id: 1,
                    def_path: synthetic.clone(),
                },
                CallEdgeType::Strong,
            ),
            (
                crate::artifacts::FunctionIdentity {
                    crate_id: 2,
                    def_path: external.clone(),
                },
                CallEdgeType::Strong,
            ),
        ]);
        callgraph.rvs_insert_M(caller.clone(), std_node);

        let seed = CapsMap::rvs_new();
        let local_scope = LocalScope::rvs_new(&BTreeSet::new());
        let impl_index = rvs_build_impl_index(&callgraph);
        let inferred = rvs_infer_caps_with_index(&callgraph, &seed, &impl_index);
        let dependents = rvs_build_inference_dependents(&callgraph, &impl_index);
        let incomplete = rvs_incomplete_inference_paths_overlay_and_dependents(
            &callgraph,
            &seed,
            &BTreeMap::new(),
            &inferred,
            &impl_index,
            &dependents,
        );
        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &seed,
            &impl_index,
            &local_scope,
        );
        let output = format!(
            "synthetic_reported={}\nexternal_reported={}\ncaller_incomplete={}\nunknown_count={}\n",
            unknown.contains_key(&synthetic),
            unknown.contains_key(&external),
            incomplete.contains(&caller),
            unknown.len(),
        );
        rvs_snapshot_BIS(
            "test_20260731_collect_std_unknown_callees_preserves_generic_uncertainty",
            &output,
        );

        assert!(!unknown.contains_key(&synthetic));
        assert!(unknown.contains_key(&external));
        assert!(incomplete.contains(&caller));
        assert_eq!(unknown.len(), 1);
    }

    #[test]
    fn test_20260716_infer_capsmap_unknown_callee_recommends_ext_only() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-unknown-callee-guidance",
            "infer-capsmap-unknown-callee-guidance",
            &[(
                "src/lib.rs",
                "pub fn rvs_call() { unsafe { fixture_dep::mystery(); } }\n",
            )],
        );
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-capsmap-unknown-callee-guidance\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "unsafe extern \"C\" { pub fn mystery(); }\n",
        )
        .unwrap();

        let error = rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps")).unwrap_err();
        let mentions_ext = error.contains("caps/ext");
        let mentions_seed = error.contains("caps/seed");
        let output = format!("mentions_ext={mentions_ext}\nmentions_seed={mentions_seed}\n");
        rvs_snapshot_BIS(
            "test_20260716_infer_capsmap_unknown_callee_recommends_ext_only",
            &output,
        );

        assert!(mentions_ext);
        assert!(!mentions_seed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260801_infer_capsmap_includes_returned_closure_body() {
        let dir = rvs_make_cargo_project_BIS(
            "infer-capsmap-returned-closure",
            "infer-capsmap-returned-closure",
            &[(
                "src/lib.rs",
                "pub fn rvs_build() { let _deferred = fixture_dep::build_deferred(); }\n",
            )],
        );
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-capsmap-returned-closure\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "pub fn build_deferred() -> impl FnOnce() {\n    move || { let _result = std::fs::read_to_string(\"deferred\"); }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/std"),
            rvs_caps_v2(&[("std::fs::read_to_string", "BI")]),
        )
        .unwrap();

        rvs_run_infer_capsmap_BIPST(&dir, Path::new("caps/deps")).unwrap();
        let deps =
            CapsMap::rvs_parse(&std::fs::read_to_string(dir.join("caps/deps")).unwrap()).unwrap();
        let info = deps
            .rvs_lookup_info("fixture_dep::build_deferred")
            .expect("never: direct external builder is emitted to deps");
        let output = format!(
            "caps={}\nbasis={}\ncompleteness={}\n",
            info.rvs_caps().rvs_letters(),
            info.rvs_basis().rvs_name(),
            info.rvs_completeness().rvs_name(),
        );
        rvs_snapshot_BIS(
            "test_20260801_infer_capsmap_includes_returned_closure_body",
            &output,
        );

        assert_eq!(info.rvs_caps().rvs_letters(), "BI");
        assert_eq!(
            info.rvs_completeness(),
            crate::capability::CapabilityCompleteness::Complete
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_std_unknown_callees_skips_local_std_crate() {
        let mut callgraph = crate::artifacts::FnGraph::rvs_new();
        let mut local_std_node = crate::artifacts::FnNode::default();
        local_std_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::rvs_external_BI"),
            },
            CallEdgeType::Strong,
        );
        callgraph.rvs_insert_M(DefPath::from("std::rvs_local"), local_std_node);

        let inferred = BTreeMap::from([(
            DefPath::from("std::rvs_local"),
            crate::capability::CapabilitySet::rvs_new(),
        )]);
        let local_scope =
            LocalScope::rvs_new(&BTreeSet::from([crate::symbols::CrateName::from("std")]));
        let impl_index = rvs_build_impl_index(&callgraph);

        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &CapsMap::rvs_new(),
            &impl_index,
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
