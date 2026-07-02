use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::capsmap;
use crate::capsmap::CapsMap;
use crate::inference::{
    rvs_build_impl_index, rvs_caps_to_string, rvs_collect_direct_external_deps, rvs_format_capsmap,
    rvs_format_unknown_callees, rvs_generate_trait_aliases_MP, rvs_infer_caps_M,
    rvs_infer_signature_caps,
};
use crate::workspace::{
    rvs_collect_callgraph_BIMS, rvs_ensure_cargo_project_BIS, rvs_load_local_crate_prefixes_BIS,
    rvs_resolve_capsmap_path, rvs_write_capsmap_result_BIS,
};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_infer_capsmap_BIMPS(
    path: &Path,
    seed_capsmap: &Path,
    output: &Option<PathBuf>,
) -> Result<(), String> {
    let local_crate_prefixes = rvs_load_local_crate_prefixes_BIS(path)?;

    let abs_seed = rvs_resolve_capsmap_path(path, seed_capsmap);
    if !abs_seed.is_dir() {
        return Err(format!(
            "capsmap path must be a directory: {}",
            abs_seed.display()
        ));
    }

    let callgraph = rvs_collect_callgraph_BIMS(
        path,
        false,
        false,
        vec![("RIVUS_CAPSMAP", abs_seed.to_string_lossy().into_owned())],
    )?;

    let seed = CapsMap::rvs_load_dir_excluding_BIS(&abs_seed, &["deps"]).unwrap_or_else(|e| {
        eprintln!("warning: caps: {e}");
        CapsMap::rvs_new()
    });

    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    let all_result = rvs_format_capsmap(&inferred);
    let cache_path = path.join("target").join("rivus-inferred-capsmap.txt");
    std::fs::write(&cache_path, &all_result)
        .map_err(|e| format!("cannot write {}: {e}", cache_path.display()))?;

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
    let deps_default_path = path.join("target").join("rivus-deps-capsmap.txt");
    match output.as_deref() {
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

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_infer_std_BIMPS(path: &Path, output: &Option<PathBuf>) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let local_crate_prefixes = rvs_load_local_crate_prefixes_BIS(path)?;
    let local_prefixes: Vec<String> = local_crate_prefixes
        .into_iter()
        .map(|name| format!("{name}::"))
        .collect();

    let callgraph = rvs_collect_callgraph_BIMS(path, true, false, vec![])?;
    let caps_dir = path.join("caps");
    let seed =
        CapsMap::rvs_load_dir_layers_BIS(&caps_dir, &["seed", "suppress"]).unwrap_or_else(|e| {
            eprintln!("warning: caps: {e}");
            CapsMap::rvs_new()
        });

    let std_crates: &[&str] = &["std::", "core::", "alloc::", "compiler_builtins::"];
    let pre_index = rvs_build_impl_index(&callgraph);
    let pre_inferred: BTreeMap<String, crate::capability::CapabilitySet> = {
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
    let std_pre_inferred: BTreeMap<String, crate::capability::CapabilitySet> = pre_inferred
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
    let impl_index = rvs_build_impl_index(&callgraph);
    let std_inferred: BTreeMap<String, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(k, _)| std_crates.iter().any(|p| k.starts_with(p)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let post_aliases = rvs_generate_trait_aliases_MP(&std_inferred, &impl_index, &callgraph);
    inferred.extend(post_aliases);

    let std_only: BTreeMap<String, crate::capability::CapabilitySet> = inferred
        .iter()
        .filter(|(name, _)| {
            !local_prefixes.iter().any(|prefix| name.starts_with(prefix))
                && std_crates.iter().any(|prefix| name.starts_with(prefix))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (func, behavior) in &callgraph {
        let is_std = std_crates.iter().any(|p| func.starts_with(p));
        if !is_std {
            continue;
        }
        for callee in &behavior.calls {
            if inferred.contains_key(callee)
                || seed.rvs_lookup(callee).is_some()
                || callgraph.contains_key(callee)
            {
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    fn rvs_make_temp_dir_BIS(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rivus-{tag}-{}-{unique}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

        let result = rvs_run_infer_capsmap_BIMPS(&dir, Path::new("caps"), &None);
        let output = format!("{result:?}");
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

        let result = rvs_run_infer_std_BIMPS(&dir, &None);
        let output = format!("{result:?}");
        rvs_snapshot_BIS("test_20260702_infer_std_rejects_workspace_root", &output);

        assert!(result.is_err());
        assert!(output.contains("missing local crate target"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
