use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::callgraph_cache::rvs_is_std_like_def_path;
use crate::capsmap::CapsMap;
use crate::cargo_targets::rvs_detect_local_crate_prefixes_for_cargo_check_BIS;
use crate::inference::{
    rvs_build_impl_index, rvs_collect_direct_external_deps, rvs_format_capsmap,
    rvs_format_unknown_callees, rvs_generate_trait_aliases, rvs_infer_caps,
    rvs_infer_signature_caps, rvs_scope_port_methods_M,
};
use crate::symbols::{CapsMapKey, CrateName, DefPath, DefPathPrefix};
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
    let local_crate_prefixes =
        rvs_detect_local_crate_prefixes_for_cargo_check_BIS(&project_path, false)?;

    let abs_seed = project_path.join("caps");
    if !rvs_validate_optional_capsmap_dir_BIS(&abs_seed)? {
        return Err(format!(
            "capsmap path must be a directory: {}",
            abs_seed.display()
        ));
    }
    let seed = CapsMap::rvs_load_dir_excluding_BIS(&abs_seed, &["deps"])
        .map_err(|e| format!("caps: {e}"))?;
    let resolved_output = rvs_resolve_output_path(&project_path, output);
    rvs_preflight_capsmap_file_BIS(&resolved_output, "deps capsmap")?;

    let mut callgraph = rvs_collect_callgraph_BIMS(
        &project_path,
        false,
        false,
        vec![("RIVUS_CAPSMAP", abs_seed.into_os_string())],
    )?;
    rvs_scope_port_methods_M(&mut callgraph, &local_crate_prefixes);

    let inferred = rvs_infer_caps(&callgraph, &seed);

    let impl_index = rvs_build_impl_index(&callgraph);
    let (direct_external_calls, unknown_callees) = rvs_collect_direct_external_deps(
        &callgraph,
        &local_crate_prefixes,
        &seed,
        &inferred,
        &impl_index,
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
    let local_crate_names =
        rvs_detect_local_crate_prefixes_for_cargo_check_BIS(&project_path, false)?;
    let local_prefixes: Vec<DefPathPrefix> = local_crate_names
        .iter()
        .map(CrateName::rvs_prefix)
        .collect();

    let caps_dir = project_path.join("caps");
    rvs_validate_optional_capsmap_dir_BIS(&caps_dir)?;
    let seed = CapsMap::rvs_load_dir_layers_BIS(&caps_dir, &["seed", "suppress"])
        .map_err(|e| format!("caps: {e}"))?;
    let mut callgraph = rvs_collect_callgraph_BIMS(&project_path, true, false, vec![])?;
    rvs_scope_port_methods_M(&mut callgraph, &local_crate_names);

    let pre_index = rvs_build_impl_index(&callgraph);
    let pre_inferred: BTreeMap<DefPath, crate::capability::CapabilitySet> = {
        let mut m = BTreeMap::new();
        for (func, behavior) in callgraph.rvs_iter() {
            if let Some(caps) = seed.rvs_lookup(func.rvs_as_str()) {
                m.insert(func.clone(), caps.clone());
            } else {
                m.insert(func.clone(), rvs_infer_signature_caps(behavior));
            }
        }
        m
    };
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

    let mut inferred = rvs_infer_caps(&callgraph, &alias_seed);
    let impl_index = rvs_build_impl_index(&callgraph);
    let std_inferred: BTreeMap<DefPath, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(k, _)| rvs_is_std_like_def_path(k.rvs_as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let post_aliases = rvs_generate_trait_aliases(&std_inferred, &impl_index, &callgraph);
    inferred.extend(post_aliases);

    let std_only: BTreeMap<DefPath, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(name, _)| {
            !local_prefixes
                .iter()
                .any(|prefix| name.rvs_starts_with(prefix))
                && rvs_is_std_like_def_path(name.rvs_as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let unknown = rvs_collect_std_unknown_callees(&callgraph, &inferred, &seed, &local_prefixes);

    if !unknown.is_empty() {
        return Err(rvs_format_unknown_callees(
            &unknown,
            "error: the following functions are called by std but have no capability data.\n\
             Add them to caps/seed with the correct capability markers:\n\n",
        ));
    }

    let result = rvs_format_capsmap(&std_only);
    let resolved_output = rvs_resolve_output_path(&project_path, output);
    rvs_write_capsmap_result_BIS(&result, &resolved_output, "std capsmap")
}

fn rvs_resolve_output_path(project_path: &Path, output_path: &Path) -> PathBuf {
    if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        project_path.join(output_path)
    }
}

fn rvs_collect_std_unknown_callees(
    callgraph: &crate::artifacts::FnGraph,
    inferred: &BTreeMap<DefPath, crate::capability::CapabilitySet>,
    seed: &CapsMap,
    local_prefixes: &[DefPathPrefix],
) -> BTreeMap<DefPath, BTreeSet<DefPath>> {
    let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
    for (func, behavior) in callgraph.rvs_iter() {
        let is_std = rvs_is_std_like_def_path(func.rvs_as_str());
        let is_local = local_prefixes
            .iter()
            .any(|prefix| func.rvs_starts_with(prefix));
        if !is_std || is_local {
            continue;
        }
        for callee in &behavior.calls {
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
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

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
        let dir = rvs_make_temp_dir_BIS("infer-std-caps-file");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-std-caps-file\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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

        let prefixes: Vec<DefPathPrefix> =
            rvs_detect_local_crate_prefixes_for_cargo_check_BIS(&dir, false)
                .unwrap()
                .into_iter()
                .map(|name| name.rvs_prefix())
                .collect();
        let output = prefixes
            .iter()
            .map(crate::symbols::DefPathPrefix::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_infer_std_local_prefixes_exclude_unchecked_std_named_tests",
            &output,
        );

        assert!(
            prefixes
                .iter()
                .any(|prefix| prefix.rvs_as_str() == "demo::")
        );
        assert!(!prefixes.iter().any(|prefix| prefix.rvs_as_str() == "std::"));
        assert!(
            !prefixes
                .iter()
                .any(|prefix| prefix.rvs_as_str() == "core::")
        );
        assert!(
            !prefixes
                .iter()
                .any(|prefix| prefix.rvs_as_str() == "alloc::")
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_infer_std_rejects_broken_caps_symlink() {
        let dir = rvs_make_temp_dir_BIS("infer-std-broken-caps-symlink");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-std-broken-caps\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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
        let dir = rvs_make_temp_dir_BIS("infer-std-invalid-seed-preflight");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-std-invalid-seed\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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
    fn test_20260706_infer_capsmap_rejects_output_directory_before_cache_write() {
        let dir = rvs_make_temp_dir_BIS("infer-capsmap-output-dir-preflight");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-capsmap-output-dir\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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
        let dir = rvs_make_temp_dir_BIS("infer-capsmap-dotdot-output");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-capsmap-dotdot-output\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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
        let dir = rvs_make_temp_dir_BIS("infer-capsmap-invalid-seed-preflight");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"infer-capsmap-invalid-seed\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_add() -> i32 { 1 }\n").unwrap();
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

        let unknown =
            rvs_collect_std_unknown_callees(&callgraph, &inferred, &CapsMap::rvs_new(), &[]);
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
        let local_prefixes = vec![crate::symbols::CrateName::from("std").rvs_prefix()];

        let unknown = rvs_collect_std_unknown_callees(
            &callgraph,
            &inferred,
            &CapsMap::rvs_new(),
            &local_prefixes,
        );
        let output = format!("unknown={unknown:?}\n");
        rvs_snapshot_BIS(
            "test_20260706_collect_std_unknown_callees_skips_local_std_crate",
            &output,
        );

        assert!(unknown.is_empty());
    }
}
