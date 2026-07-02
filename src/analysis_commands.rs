use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::inference::{
    rvs_build_impl_index, rvs_caps_to_string, rvs_infer_caps_M, rvs_resolve_impl_majority_caps_M,
};
use crate::rename;
use crate::workspace::{
    rvs_ensure_cargo_project_BIS, rvs_load_callgraph_and_caps_BIMS,
    rvs_load_local_crate_prefixes_BIS,
};

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    let local_crate_names = rvs_load_local_crate_prefixes_BIS(path)?;
    let (callgraph, seed) = rvs_load_callgraph_and_caps_BIMS(path)?;
    let inferred = rvs_infer_caps_M(&callgraph, &seed);
    let local_prefixes: Vec<String> = local_crate_names
        .into_iter()
        .map(|name| format!("{name}::"))
        .collect();
    let root_main_paths: BTreeSet<String> = local_prefixes
        .iter()
        .map(|prefix| format!("{prefix}main"))
        .collect();

    let mut rename_map: HashMap<String, String> = HashMap::new();
    for (full_path, caps) in &inferred {
        let Some(relative_path) = local_prefixes
            .iter()
            .find_map(|prefix| full_path.strip_prefix(prefix))
        else {
            continue;
        };
        let short_name = relative_path.rsplit("::").next().unwrap_or(relative_path);
        if short_name.starts_with("rvs_") {
            continue;
        }
        if short_name.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        if root_main_paths.contains(full_path) {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_test) {
            continue;
        }
        if callgraph.get(full_path).is_some_and(|b| b.is_trait_impl) {
            continue;
        }
        let caps_str = rvs_caps_to_string(caps);
        let new_name = if caps_str.is_empty() {
            format!("rvs_{short_name}")
        } else {
            format!("rvs_{short_name}_{caps_str}")
        };
        rename_map.insert(relative_path.to_string(), new_name);
    }

    if rename_map.is_empty() {
        println!("No functions to annotate.");
        return Ok(());
    }

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

    #[test]
    fn test_20260702_annotate_renames_nested_main_helper() {
        let dir = rvs_make_temp_dir_BIS("annotate-nested-main");
        let cargo_toml =
            "[package]\nname = \"annotate-main-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "mod cli { pub fn main() {} }\n\nfn main() { cli::main(); }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_main_demo::main": {
    "calls": ["annotate_main_demo::cli::main"],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "annotate_main_demo::cli::main": {
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
        rvs_snapshot_BIS("test_20260702_annotate_renames_nested_main_helper", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_main()"));
        assert!(source.contains("cli::rvs_main();"));
        assert!(source.contains("fn main() {"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_renames_conflicting_duplicate_names() {
        let dir = rvs_make_temp_dir_BIS("annotate-duplicate-name");
        let cargo_toml = "[package]\nname = \"annotate-duplicate-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub mod a { pub fn parse() {} }\npub mod b { pub fn parse(_x: &mut u8) {} }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_duplicate_demo::a::parse": {
    "calls": [],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "annotate_duplicate_demo::b::parse": {
    "calls": [],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": true,
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
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260702_annotate_renames_conflicting_duplicate_names",
            &source,
        );

        assert!(
            result.is_ok(),
            "annotate should rename duplicate names by relative path: {result:?}"
        );
        assert!(source.contains("pub fn rvs_parse() {}"));
        assert!(source.contains("pub fn rvs_parse_M(_x: &mut u8) {}"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_surfaces_callgraph_collection_error() {
        let dir = rvs_make_temp_dir_BIS("annotate-callgraph-error");
        let cargo_toml = "[package]\nname = \"annotate-callgraph-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn parse() {}\n").unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            "{ not valid json }\n",
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let output = format!("{result:?}");
        rvs_snapshot_BIS(
            "test_20260702_annotate_surfaces_callgraph_collection_error",
            &output,
        );

        assert!(
            result.is_err(),
            "annotate should return the callgraph load failure"
        );
        assert!(output.contains("invalid callgraph JSON"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
