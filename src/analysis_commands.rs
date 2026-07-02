use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::inference::{
    rvs_build_impl_index, rvs_caps_to_string, rvs_infer_caps_M, rvs_resolve_impl_majority_caps_M,
};
use crate::rename;
use crate::workspace::{
    rvs_detect_local_crate_prefixes_BIS, rvs_ensure_cargo_project_BIS,
    rvs_load_callgraph_and_caps_BIMS,
};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;

    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);
    let local_prefixes: Vec<String> = rvs_detect_local_crate_prefixes_BIS(path)?
        .into_iter()
        .map(|name| format!("{name}::"))
        .collect();

    let mut renames: Vec<(String, String)> = Vec::new();
    let mut skip_names: HashSet<String> = HashSet::new();
    for (full_path, caps) in &inferred {
        if !local_prefixes
            .iter()
            .any(|prefix| full_path.starts_with(prefix))
        {
            continue;
        }
        let short_name = full_path.rsplit("::").next().unwrap_or(full_path);
        if short_name.starts_with("rvs_") {
            continue;
        }
        if short_name.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        if short_name == "main" {
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
    rvs_ensure_cargo_project_BIS(path)?;

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
                        rvs_resolve_impl_majority_caps_M(callee, &impl_index, &inferred, &callgraph)
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
    fn test_20260702_annotate_uses_bin_crate_prefix() {
        let dir = rvs_make_temp_dir_BIS("annotate-bin-prefix");
        let cargo_toml = r#"[package]
name = "annotate-prefix-demo"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "cargo-rivus"
path = "src/main.rs"
"#;
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() { parse(); }\n\nfn parse() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "cargo_rivus::main": {
    "calls": ["cargo_rivus::parse"],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "cargo_rivus::parse": {
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
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS("test_20260702_annotate_uses_bin_crate_prefix", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("fn rvs_parse()"));
        assert!(source.contains("rvs_parse();"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
