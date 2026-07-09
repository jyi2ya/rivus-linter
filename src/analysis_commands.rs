use std::collections::HashMap;
use std::path::Path;

use crate::artifacts::{FnGraph, FnSource};
use crate::inference::{
    FnContractDiff, rvs_build_graph_impl_index, rvs_caps_to_string,
    rvs_collect_local_contract_diffs_M, rvs_contract_diff_is_enforced,
    rvs_resolve_graph_impl_majority_caps_M,
};
use crate::rename;
use crate::symbols::{CrateName, DefPath, FnName};

#[cfg(test)]
use crate::symbols::RelativeFnPath;
use crate::workspace::{
    rvs_collect_callgraph_and_caps_BIMS, rvs_detect_local_crate_prefixes_for_cargo_check_BIS,
    rvs_ensure_cargo_project_BIS, rvs_function_matches_local_prefix,
    rvs_load_callgraph_and_caps_for_function_BIMS, rvs_load_local_crate_prefixes_BIS,
};

fn rvs_normalize_source_for_project_BIS(
    source: &FnSource,
    project_path: &Path,
) -> Result<FnSource, String> {
    let file = if source.file.is_absolute() {
        source.file.clone()
    } else {
        project_path.join(&source.file)
    };
    let file = file
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize source '{}': {e}", file.display()))?;
    Ok(FnSource::rvs_new(file, source.name_start, source.name_end))
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_annotate_BIMPS(path: &Path) -> Result<(), String> {
    let local_crate_names = rvs_detect_local_crate_prefixes_for_cargo_check_BIS(path, false)?;
    let (mut callgraph, seed) = rvs_collect_callgraph_and_caps_BIMS(path, false)?;
    let diffs = rvs_collect_local_contract_diffs_M(&mut callgraph, &seed, &local_crate_names);
    let mut rename_map: HashMap<FnSource, FnName> = HashMap::new();
    let mut skipped_without_source = 0usize;
    for diff in diffs {
        let Some(expected_name) = diff.expected_name.as_ref() else {
            continue;
        };
        if !diff.rvs_has_name_mismatch() || expected_name == &diff.actual_name {
            continue;
        }
        let Some(node) = callgraph.rvs_get(diff.def_path.rvs_as_str()) else {
            skipped_without_source += 1;
            eprintln!(
                "warning: skipping annotate candidate '{}' because callgraph metadata is missing",
                diff.def_path.rvs_as_str()
            );
            continue;
        };
        let sources: Vec<_> = node.sources.iter().cloned().collect();
        if sources.is_empty() {
            skipped_without_source += 1;
            eprintln!(
                "warning: skipping annotate candidate '{}' because it has no real source location metadata",
                diff.def_path.rvs_as_str()
            );
            continue;
        }
        for source in sources {
            let source = rvs_normalize_source_for_project_BIS(&source, path)?;
            if let Some(existing_expected_name) = rename_map.get(&source) {
                if existing_expected_name != expected_name {
                    return Err(format!(
                        "annotate candidate source '{}:{}..{}' has conflicting expected names ('{}' vs '{}')",
                        source.file.display(),
                        source.name_start,
                        source.name_end,
                        existing_expected_name,
                        expected_name
                    ));
                }
                continue;
            }
            rename_map.insert(source, expected_name.clone());
        }
    }

    if rename_map.is_empty() {
        if skipped_without_source > 0 {
            println!(
                "No functions to annotate (skipped {skipped_without_source} candidate(s) without source metadata)."
            );
            return Ok(());
        }
        println!("No functions to annotate.");
        return Ok(());
    }

    let stats = rename::rvs_apply_ra_source_renames_BIS(path, &rename_map)?;
    if stats.matched_functions == 0 {
        return Err(format!(
            "annotate found {} candidate function(s), but none were renamed by rust-analyzer",
            rename_map.len()
        ));
    }
    if stats.matched_functions < rename_map.len() {
        return Err(format!(
            "annotate only renamed {} of {} candidate function(s)",
            stats.matched_functions,
            rename_map.len()
        ));
    }

    println!(
        "Annotate complete: renamed {} function(s) in {} file(s).",
        stats.matched_functions, stats.files_changed
    );
    Ok(())
}

fn rvs_is_std_like_query(function: &str) -> bool {
    function.starts_with("std::")
        || function.starts_with("core::")
        || function.starts_with("alloc::")
        || function.starts_with("compiler_builtins::")
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_why_BIMPS(function: &str, path: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;

    let local_crate_names = if rvs_is_std_like_query(function) {
        let loaded = rvs_load_local_crate_prefixes_BIS(path);
        match loaded {
            Ok(names) if rvs_function_matches_local_prefix(function, &names) => names,
            Ok(_) | Err(_) => std::collections::BTreeSet::new(),
        }
    } else {
        rvs_load_local_crate_prefixes_BIS(path)?
    };
    let (mut callgraph, seed) = rvs_load_callgraph_and_caps_for_function_BIMS(path, function)?;
    let diffs = rvs_collect_local_contract_diffs_M(&mut callgraph, &seed, &local_crate_names);
    let inferred = callgraph.rvs_expected_public_caps_map();
    let impl_index = rvs_build_graph_impl_index(&callgraph);

    let Some(behavior) = callgraph.rvs_get(function) else {
        let candidates: Vec<&DefPath> = callgraph
            .rvs_keys()
            .filter(|k| k.rvs_contains(function))
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
        return Err(format!(
            "function '{function}' not found; see suggestions above"
        ));
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
    for line in
        rvs_format_enforced_contract_diff_summary(&callgraph, &diffs, &local_crate_names, function)
    {
        println!("  {line}");
    }
    println!();

    if !behavior.has_body {
        println!(
            "  {}",
            rvs_callee_absence_message(false, behavior.is_synthetic)
        );
        return Ok(());
    }

    if behavior.calls.is_empty() {
        println!(
            "  {}",
            rvs_callee_absence_message(true, behavior.is_synthetic)
        );
        return Ok(());
    }

    let mut callees: Vec<(&DefPath, Option<crate::capability::CapabilitySet>)> = behavior
        .calls
        .iter()
        .map(|callee| {
            let caps = inferred
                .get(callee)
                .cloned()
                .or_else(|| seed.rvs_lookup(callee.rvs_as_str()).cloned())
                .or_else(|| {
                    if !callee.rvs_as_str().contains('@') {
                        rvs_resolve_graph_impl_majority_caps_M(
                            callee,
                            &impl_index,
                            &inferred,
                            &callgraph,
                        )
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

fn rvs_callee_absence_message(had_collected_body: bool, is_synthetic: bool) -> &'static str {
    if had_collected_body {
        "(no callees)"
    } else if is_synthetic {
        "(function body not collected; callees unknown)"
    } else {
        "(function has no body; callees intentionally absent)"
    }
}

fn rvs_format_contract_diff_summary(diff: &FnContractDiff) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(expected_name) = diff.expected_name.as_ref()
        && expected_name != &diff.actual_name
    {
        lines.push(format!("expected name: {expected_name}"));
    }
    lines.push(format!(
        "declared caps: {}",
        rvs_format_optional_caps(diff.declared_public_caps.as_ref())
    ));
    lines.push(format!(
        "expected caps: {}",
        rvs_format_optional_caps(diff.expected_public_caps.as_ref())
    ));
    if let (Some(declared), Some(expected)) = (
        diff.declared_public_caps.as_ref(),
        diff.expected_public_caps.as_ref(),
    ) {
        let missing: Vec<_> = expected
            .rvs_iter()
            .filter(|cap| !declared.rvs_contains(*cap))
            .map(|cap| cap.rvs_as_char())
            .collect();
        let extra: Vec<_> = declared
            .rvs_iter()
            .filter(|cap| !expected.rvs_contains(*cap))
            .map(|cap| cap.rvs_as_char())
            .collect();
        if !missing.is_empty() {
            lines.push(format!(
                "missing caps: {}",
                missing.iter().copied().collect::<String>()
            ));
        }
        if !extra.is_empty() {
            lines.push(format!(
                "extra declared caps: {}",
                extra.iter().copied().collect::<String>()
            ));
        }
    }
    let mismatch_labels: Vec<&str> = diff
        .rvs_mismatch_kinds()
        .into_iter()
        .map(|kind| kind.rvs_as_str())
        .collect();
    if !mismatch_labels.is_empty() {
        lines.push(format!("mismatches: {}", mismatch_labels.join(", ")));
    }
    lines
}

fn rvs_format_optional_caps(caps: Option<&crate::capability::CapabilitySet>) -> String {
    match caps {
        Some(caps) => {
            let text = rvs_caps_to_string(caps);
            if text.is_empty() {
                "(pure)".to_string()
            } else {
                text
            }
        }
        None => "(not declared)".to_string(),
    }
}

fn rvs_format_enforced_contract_diff_summary(
    graph: &FnGraph,
    diffs: &[FnContractDiff],
    local_crate_names: &std::collections::BTreeSet<CrateName>,
    function: &str,
) -> Vec<String> {
    let Some(diff) = diffs.iter().find(|diff| {
        diff.def_path.rvs_as_str() == function
            && rvs_contract_diff_is_enforced(graph, diff, local_crate_names)
    }) else {
        return Vec::new();
    };
    rvs_format_contract_diff_summary(diff)
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
    fn test_20260703_format_contract_diff_summary() {
        let diff = FnContractDiff {
            def_path: DefPath::from("demo::rvs_fetch_ABI"),
            actual_name: FnName::from("rvs_fetch_ABI"),
            expected_name: Some(FnName::from("rvs_fetch_P")),
            declared_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("ABI")),
            expected_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("AP")),
        };
        let lines = rvs_format_contract_diff_summary(&diff);
        let none_caps = rvs_format_optional_caps(None);
        rvs_snapshot_BIS(
            "test_20260703_format_contract_diff_summary",
            &format!("{}\nnone={none_caps}\n", lines.join("\n")),
        );

        assert_eq!(
            lines,
            vec![
                "expected name: rvs_fetch_P".to_string(),
                "declared caps: ABI".to_string(),
                "expected caps: AP".to_string(),
                "missing caps: P".to_string(),
                "extra declared caps: BI".to_string(),
                "mismatches: name_mismatch, missing_port".to_string(),
            ]
        );
    }

    #[test]
    fn test_20260706_normalize_source_for_project() {
        let dir = rvs_make_temp_dir_BIS("normalize-source");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "fn parse() {}\n").unwrap();
        let source = FnSource::rvs_new(std::path::PathBuf::from("src/lib.rs"), 3, 8);

        let normalized = rvs_normalize_source_for_project_BIS(&source, &dir).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_normalize_source_for_project",
            &format!(
                "abs={}\nstart={}\nend={}\n",
                normalized.file.is_absolute(),
                normalized.name_start,
                normalized.name_end
            ),
        );

        assert!(normalized.file.is_absolute());
        assert_eq!(normalized.name_start, 3);
        assert_eq!(normalized.name_end, 8);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_normalize_source_reports_missing_file() {
        let dir = rvs_make_temp_dir_BIS("normalize-source-missing");
        let source = FnSource::rvs_new(std::path::PathBuf::from("src/missing.rs"), 3, 8);

        let result = rvs_normalize_source_for_project_BIS(&source, &dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_normalize_source_reports_missing_file",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260703_why_contract_summary_skips_root_main() {
        let mut graph = FnGraph::rvs_new();
        let node = crate::artifacts::FnNode {
            expected_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("BI")),
            ..crate::artifacts::FnNode::default()
        };
        graph.rvs_insert_M(DefPath::from("demo::main"), node);
        let diff = FnContractDiff {
            def_path: DefPath::from("demo::main"),
            actual_name: FnName::from("main"),
            expected_name: None,
            declared_public_caps: None,
            expected_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("BI")),
        };
        let lines = rvs_format_enforced_contract_diff_summary(
            &graph,
            &[diff],
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
            "demo::main",
        );
        rvs_snapshot_BIS(
            "test_20260703_why_contract_summary_skips_root_main",
            &format!("lines={lines:?}\n"),
        );

        assert!(lines.is_empty());
    }

    #[test]
    fn test_20260703_callee_absence_message_distinguishes_synthetic_nodes() {
        let output = format!(
            "collected={}\nbodyless={}\nsynthetic={}\n",
            rvs_callee_absence_message(true, false),
            rvs_callee_absence_message(false, false),
            rvs_callee_absence_message(false, true),
        );
        rvs_snapshot_BIS(
            "test_20260703_callee_absence_message_distinguishes_synthetic_nodes",
            &output,
        );

        assert_eq!(rvs_callee_absence_message(true, false), "(no callees)");
        assert_eq!(
            rvs_callee_absence_message(false, false),
            "(function has no body; callees intentionally absent)"
        );
        assert_eq!(
            rvs_callee_absence_message(false, true),
            "(function body not collected; callees unknown)"
        );
    }

    #[test]
    fn test_20260705_why_std_like_works_in_workspace_root() {
        let dir = rvs_make_temp_dir_BIS("why-std-workspace-root");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
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
        std::fs::write(
            dir.join("target/rivus-std-capsmap.txt"),
            "std::fs::rvs_read_BI=BI\n",
        )
        .unwrap();

        let result = rvs_run_why_BIMPS("std::fs::rvs_read_BI", &dir);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260705_why_std_like_works_in_workspace_root",
            &output,
        );

        assert!(rvs_is_std_like_query("std::fs::rvs_read_BI"));
        assert!(!rvs_is_std_like_query("demo::rvs_read_BI"));
        assert!(result.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_why_inexact_match_returns_error() {
        let dir = rvs_make_temp_dir_BIS("why-inexact-match");
        let cargo_toml =
            "[package]\nname = \"why-inexact-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_parse() -> i32 { 1 }\n").unwrap();

        let result = rvs_run_why_BIMPS("parse", &dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260706_why_inexact_match_returns_error", &output);

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
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
    "has_body": true,
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
    "has_body": true,
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
    fn test_20260709_annotate_skips_integration_test_targets() {
        let dir = rvs_make_temp_dir_BIS("annotate-skip-integration-tests");
        let cargo_toml = "[package]\nname = \"annotate-skip-integration-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn parse(values: &mut Vec<u8>) { values.push(1); }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests/fixtures")).unwrap();
        std::fs::write(
            dir.join("tests/fixtures/mod.rs"),
            "pub struct TestServer;\n\nimpl TestServer {\n    pub fn url(&self, values: &mut Vec<u8>) { values.push(1); }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("tests/upload_files.rs"),
            "mod fixtures;\n\n#[test]\nfn integration_fixture_keeps_plain_name() {\n    let server = fixtures::TestServer;\n    let mut values = Vec::new();\n    server.url(&mut values);\n}\n",
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let fixture = std::fs::read_to_string(dir.join("tests/fixtures/mod.rs")).unwrap();
        let output = format!("-- src/lib.rs --\n{source}\n-- tests/fixtures/mod.rs --\n{fixture}");
        rvs_snapshot_BIS(
            "test_20260709_annotate_skips_integration_test_targets",
            &output,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse_M"));
        assert!(fixture.contains("pub fn url"));
        assert!(!fixture.contains("rvs_url"));

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
    "has_body": true,
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
    "has_body": true,
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
    "has_body": true,
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
    "has_body": true,
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
    fn test_20260703_annotate_renames_existing_rvs_wrong_suffix() {
        let dir = rvs_make_temp_dir_BIS("annotate-existing-rvs-suffix");
        let cargo_toml =
            "[package]\nname = \"annotate-rvs-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub trait ApiClient { fn rvs_fetch_ABI(&self) -> i32 { 1 } }\npub fn rvs_use<C: ApiClient>(client: &C) -> i32 { client.rvs_fetch_ABI() }\n",
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260703_annotate_renames_existing_rvs_wrong_suffix",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("fn rvs_fetch_P(&self)"));
        assert!(source.contains("client.rvs_fetch_P()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_uppercase_function() {
        let dir = rvs_make_temp_dir_BIS("annotate-uppercase-function");
        let cargo_toml = "[package]\nname = \"annotate-uppercase-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn Foo() -> i32 { 1 }\n").unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_uppercase_demo::Foo": {
    "calls": [],
    "has_body": true,
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
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS("test_20260704_annotate_renames_uppercase_function", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_Foo()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_errors_when_candidates_match_no_symbols() {
        let dir = rvs_make_temp_dir_BIS("annotate-unmatched-candidate");
        let cargo_toml = "[package]\nname = \"annotate-unmatched-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn existing() -> i32 { 1 }\n").unwrap();
        let rename_map =
            HashMap::from([(RelativeFnPath::from("missing"), FnName::from("rvs_missing"))]);
        let result = rename::rvs_apply_ra_renames_BIS(&dir, &rename_map);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_annotate_errors_when_candidates_match_no_symbols",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("did not match any rust-analyzer symbol"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_errors_on_partial_rename() {
        let dir = rvs_make_temp_dir_BIS("annotate-partial-rename");
        let cargo_toml = "[package]\nname = \"annotate-partial-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn parse() -> i32 { 1 }\n").unwrap();
        let rename_map = HashMap::from([
            (RelativeFnPath::from("parse"), FnName::from("rvs_parse")),
            (RelativeFnPath::from("missing"), FnName::from("rvs_missing")),
        ]);
        let result = rename::rvs_apply_ra_renames_BIS(&dir, &rename_map);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let output = format!("{result:?}\n{source}");
        rvs_snapshot_BIS("test_20260704_annotate_errors_on_partial_rename", &output);

        assert!(result.is_err());
        assert!(output.contains("did not match any rust-analyzer symbol"));
        assert!(source.contains("pub fn parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_out_of_line_module_function() {
        let dir = rvs_make_temp_dir_BIS("annotate-out-of-line-module");
        let cargo_toml =
            "[package]\nname = \"annotate-module-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub mod api;\n").unwrap();
        std::fs::write(dir.join("src/api.rs"), "pub fn parse() -> i32 { 1 }\n").unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_module_demo::api::parse": {
    "calls": [],
    "has_body": true,
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
        let source = std::fs::read_to_string(dir.join("src/api.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260704_annotate_renames_out_of_line_module_function",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_renames_path_attribute_module_function() {
        let dir = rvs_make_temp_dir_BIS("annotate-path-attribute-module");
        let cargo_toml = "[package]\nname = \"annotate-path-attr-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#[path = \"wire.rs\"]\npub mod api;\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/wire.rs"), "pub fn parse() -> i32 { 1 }\n").unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let source = std::fs::read_to_string(dir.join("src/wire.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_renames_path_attribute_module_function",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_renames_lib_and_main_same_name_functions() {
        let dir = rvs_make_temp_dir_BIS("annotate-lib-main-same-name");
        let cargo_toml = "[package]\nname = \"annotate-samename-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn parse() -> i32 { 1 }\n").unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn parse() -> i32 { 2 }\n\nfn main() { let _ = parse(); }\n",
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let lib_source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let main_source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_renames_lib_and_main_same_name_functions",
            &format!("lib:\n{lib_source}\nmain:\n{main_source}"),
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(lib_source.contains("pub fn rvs_parse()"));
        assert!(main_source.contains("fn rvs_parse()"));
        assert!(main_source.contains("rvs_parse();"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_skips_macro_generated_function_without_source() {
        let dir = rvs_make_temp_dir_BIS("annotate-macro-generated-function");
        let cargo_toml =
            "[package]\nname = \"annotate-macro-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "macro_rules! make_parse { () => { pub fn parse() -> i32 { 1 } }; }\nmake_parse!();\n",
        )
        .unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_skips_macro_generated_function_without_source",
            &source,
        );

        assert!(
            result.is_ok(),
            "annotate should skip source-less macro function: {result:?}"
        );
        assert!(source.contains("pub fn parse()"));
        assert!(!source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_inherent_method() {
        let dir = rvs_make_temp_dir_BIS("annotate-inherent-method");
        let cargo_toml =
            "[package]\nname = \"annotate-method-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub struct User;\nimpl User { pub fn new() -> Self { Self } }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_method_demo::User::new": {
    "calls": [],
    "has_body": true,
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
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS("test_20260704_annotate_renames_inherent_method", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_new()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_generic_inherent_method() {
        let dir = rvs_make_temp_dir_BIS("annotate-generic-inherent-method");
        let cargo_toml = "[package]\nname = \"annotate-generic-method-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub struct User<T>(T);\nimpl<T> User<T> { pub fn new(value: T) -> Self { Self(value) } }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_generic_method_demo::User::new": {
    "calls": [],
    "has_body": true,
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
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260704_annotate_renames_generic_inherent_method",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_new(value: T)"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_surfaces_callgraph_collection_error() {
        let dir = rvs_make_temp_dir_BIS("annotate-callgraph-error");
        let cargo_toml = "[package]\nname = \"annotate-callgraph-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn parse( {\n").unwrap();

        let result = rvs_run_annotate_BIMPS(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_annotate_surfaces_callgraph_collection_error",
            &output,
        );

        assert!(
            result.is_err(),
            "annotate should return the fresh collection failure"
        );
        assert!(output.contains("cargo check failed"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
