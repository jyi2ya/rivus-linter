use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::inference::{
    rvs_build_impl_index, rvs_caps_to_string, rvs_infer_caps_M, rvs_resolve_impl_union_M,
};
use crate::rename;
use crate::workspace::{
    rvs_detect_crate_name_BIS, rvs_ensure_project_dir_BS, rvs_load_callgraph_and_caps_BIMS,
};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;

    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);

    let workspace_name = rvs_detect_crate_name_BIS(path)?;

    let mut renames: Vec<(String, String)> = Vec::new();
    let mut skip_names: HashSet<String> = HashSet::new();
    for (full_path, caps) in &inferred {
        if !full_path.starts_with(&format!("{workspace_name}::")) {
            continue;
        }
        let short_name = full_path.rsplit("::").next().unwrap_or(full_path);
        if short_name.starts_with("rvs_") {
            continue;
        }
        if short_name.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        if full_path == &format!("{workspace_name}::main") {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_test) {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_trait_impl) {
            skip_names.insert(short_name.to_string());
            continue;
        }
        let caps_str = rvs_caps_to_string(caps);
        let new_name = if caps_str.is_empty() {
            format!("rvs_{short_name}")
        } else {
            format!("rvs_{short_name}_{caps_str}")
        };
        renames.push((short_name.to_string(), new_name));
    }

    renames.retain(|(name, _)| !skip_names.contains(name));
    renames.sort();
    renames.dedup();

    if renames.is_empty() {
        println!("No functions to annotate.");
        return Ok(());
    }

    let rename_map: HashMap<String, String> = renames.into_iter().collect();
    let files_changed = rename::rvs_apply_ra_renames_BIS(path, &rename_map)?;

    println!(
        "Annotate complete: renamed {} function(s) in {} file(s).",
        rename_map.len(),
        files_changed
    );
    Ok(())
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_why_BIMPS(function: &str, path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BS(path)?;

    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);
    let impl_index = rvs_build_impl_index(&callgraph);

    let Some(behavior) = callgraph.get(function) else {
        let candidates: Vec<&String> = callgraph
            .keys()
            .filter(|k| k.contains(function))
            .take(10)
            .collect();
        if candidates.is_empty() {
            return Err(format!("function '{function}' not found in callgraph"));
        }
        eprintln!("Exact match not found. Did you mean:");
        for c in &candidates {
            let caps_str = inferred
                .get(*c)
                .map(|cs| {
                    let s = rvs_caps_to_string(cs);
                    if s.is_empty() {
                        " (pure)".to_string()
                    } else {
                        format!(" = {s}")
                    }
                })
                .unwrap_or_else(|| " (unknown)".to_string());
            eprintln!("  {c}{caps_str}");
        }
        return Ok(());
    };

    let own_caps = inferred.get(function);
    let caps_str = match own_caps {
        Some(cs) => {
            let s = rvs_caps_to_string(cs);
            if s.is_empty() {
                " (pure)".to_string()
            } else {
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(" = {s} ({desc})")
            }
        }
        None => " (not in inferred)".to_string(),
    };
    println!("{function}{caps_str}");
    println!();

    if behavior.calls.is_empty() {
        println!("  (no callees)");
        return Ok(());
    }

    let mut callees: Vec<(&String, Option<crate::capability::CapabilitySet>)> = behavior
        .calls
        .iter()
        .map(|callee| {
            let caps = inferred
                .get(callee)
                .cloned()
                .or_else(|| seed.rvs_lookup(callee).cloned())
                .or_else(|| {
                    if !callee.contains('@') {
                        rvs_resolve_impl_union_M(callee, &impl_index, &inferred, &callgraph)
                    } else {
                        None
                    }
                });
            (callee, caps)
        })
        .collect();
    callees.sort_by(|a, b| a.0.cmp(b.0));

    println!("  callees:");
    for (callee, caps) in &callees {
        let s = match caps {
            Some(cs) if !cs.rvs_is_empty() => {
                let chars = rvs_caps_to_string(cs);
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{chars} ({desc})")
            }
            Some(_) => "(pure)".to_string(),
            None => "(unknown)".to_string(),
        };
        println!("    {callee}: {s}");
    }

    Ok(())
}
