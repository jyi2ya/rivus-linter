use std::collections::BTreeSet;
use std::path::Path;

use crate::artifacts::{self, FnGraph};
use crate::function_classification::LocalScope;
use crate::symbols::CrateName;

pub(crate) fn rvs_merge_std_like_callgraph_M(target: &mut FnGraph, source: FnGraph) {
    rvs_merge_std_like_callgraph_with_local_prefixes_M(target, source, &BTreeSet::new());
}

pub(crate) fn rvs_merge_std_like_callgraph_with_local_prefixes_M(
    target: &mut FnGraph,
    source: FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let local_scope = LocalScope::rvs_new(local_crate_names);
    for (path, node) in source.nodes {
        if rvs_is_std_like_def_path(path.rvs_as_str()) && !local_scope.rvs_contains(&path) {
            target.rvs_merge_node_M(path, node);
        }
    }
}

pub(crate) fn rvs_is_std_like_def_path(function: &str) -> bool {
    function.starts_with("std::")
        || function.starts_with("core::")
        || function.starts_with("alloc::")
        || function.starts_with("compiler_builtins::")
}

pub(crate) fn rvs_merge_callgraph_dir_BIS(
    cg_dir: &Path,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    let mut artifacts = Vec::new();
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
        let partial = artifacts::rvs_parse_callgraph_json_S(&json_str)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        artifacts.push(partial);
    }
    if json_paths.is_empty() {
        return Err(format!(
            "no callgraph JSON artifacts found in {}",
            cg_dir.display()
        ));
    }
    let merged = FnGraph::rvs_merge_artifacts(artifacts, local_crate_names)?;
    Ok(merged)
}
