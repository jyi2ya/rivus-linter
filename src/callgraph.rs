use std::collections::BTreeSet;

use crate::artifacts::{CallgraphArtifactError, FnGraph};
use crate::function_classification::LocalScope;
use crate::symbols::{CrateName, rvs_strip_identity_markers};

#[cfg(test)]
pub(crate) fn rvs_merge_std_like_callgraph_M(
    target: &mut FnGraph,
    source: &FnGraph,
) -> Result<(), CallgraphArtifactError> {
    rvs_merge_std_like_callgraph_with_local_prefixes_M(target, source, &BTreeSet::new())
}

pub(crate) fn rvs_merge_std_like_callgraph_with_local_prefixes_M(
    target: &mut FnGraph,
    source: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<(), CallgraphArtifactError> {
    let local_scope = LocalScope::rvs_new(local_crate_names);
    let source_is_legacy = source.rvs_is_legacy();
    if target.rvs_is_legacy() != source_is_legacy {
        return Err(CallgraphArtifactError::MixedArtifactFormats {
            legacy_count: usize::from(target.rvs_is_legacy()) + usize::from(source_is_legacy),
            current_count: usize::from(!target.rvs_is_legacy()) + usize::from(!source_is_legacy),
        });
    }
    let selected = source
        .nodes
        .iter()
        .filter(|(path, _)| {
            rvs_is_std_like_def_path(path.rvs_as_str()) && !local_scope.rvs_contains(path)
        })
        .collect::<Vec<_>>();
    let mut merged = target.clone();
    for (path, node) in selected {
        merged.rvs_merge_node_M(path, node)?;
    }
    *target = merged;
    Ok(())
}

pub(crate) fn rvs_filter_std_like_callgraph_M(
    graph: &mut FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let local_scope = LocalScope::rvs_new(local_crate_names);
    graph.nodes.retain(|path, _| {
        rvs_is_std_like_def_path(path.rvs_as_str()) && !local_scope.rvs_contains(path)
    });
}

pub(crate) fn rvs_is_std_like_def_path(function: &str) -> bool {
    let user_path = rvs_strip_identity_markers(function);
    user_path.starts_with("std::")
        || user_path.starts_with("core::")
        || user_path.starts_with("alloc::")
        || user_path.starts_with("compiler_builtins::")
}
