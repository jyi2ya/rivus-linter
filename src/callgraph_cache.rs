use std::collections::BTreeSet;
use std::path::Path;

use crate::artifacts::{self, FnGraph};
use crate::cargo_targets::rvs_function_matches_local_prefix;
use crate::symbols::CrateName;

pub(crate) fn rvs_merge_std_like_callgraph_M(target: &mut FnGraph, source: FnGraph) {
    rvs_merge_std_like_callgraph_with_local_prefixes_M(target, source, &BTreeSet::new());
}

pub(crate) fn rvs_merge_std_like_callgraph_with_local_prefixes_M(
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

pub(crate) fn rvs_is_std_like_def_path(function: &str) -> bool {
    function.starts_with("std::")
        || function.starts_with("core::")
        || function.starts_with("alloc::")
        || function.starts_with("compiler_builtins::")
}

pub(crate) fn rvs_merge_callgraph_dir_BIS(cg_dir: &Path) -> Result<FnGraph, String> {
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

pub(crate) fn rvs_reject_stale_callgraph_json(path: &Path, json: &str) -> Result<(), String> {
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
