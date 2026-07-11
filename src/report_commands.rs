use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::artifacts::FnGraph;
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet, ParsedFunctionName};
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::inference::{
    FnContractDiff, FnContractMismatch, FnContractMismatchKind, PreparedLocalAnalysis,
    rvs_collect_contract_mismatch_items, rvs_collect_enforced_contract_diffs,
    rvs_summarize_contract_mismatch_items,
};
use crate::symbols::CrateName;
use crate::workspace::{
    rvs_collect_callgraph_and_caps_BIMS, rvs_ensure_cargo_project_BIS,
    rvs_load_local_crate_prefixes_BIS,
};

#[derive(Debug, Clone, Default)]
struct CapStats {
    fn_count: usize,
    line_count: usize,
}

#[derive(Debug, Clone)]
struct Report {
    by_capability: BTreeMap<Capability, CapStats>,
    pure_fn_count: usize,
    pure_line_count: usize,
    good_fn_count: usize,
    good_line_count: usize,
    ok_fn_count: usize,
    ok_line_count: usize,
    total_fn_count: usize,
    total_line_count: usize,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Capability Report")?;
        writeln!(f, "{:-<60}", "")?;
        writeln!(
            f,
            "Total: {} functions, {} lines",
            self.total_fn_count, self.total_line_count
        )?;
        writeln!(f, "{:-<60}", "")?;

        if self.total_fn_count == 0 {
            writeln!(f, "(no rvs_ functions found)")?;
            return Ok(());
        }

        let bar_width = 30;
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        rows.push(("(ok)".to_string(), self.ok_fn_count, self.ok_line_count));
        rows.push((
            "(good)".to_string(),
            self.good_fn_count,
            self.good_line_count,
        ));
        rows.push((
            "(pure)".to_string(),
            self.pure_fn_count,
            self.pure_line_count,
        ));

        for cap in [
            Capability::A,
            Capability::B,
            Capability::I,
            Capability::M,
            Capability::P,
            Capability::S,
            Capability::T,
            Capability::U,
        ] {
            if let Some(stats) = self.by_capability.get(&cap) {
                rows.push((cap.to_string(), stats.fn_count, stats.line_count));
            }
        }
        rows.sort_by_key(|b| std::cmp::Reverse(b.2));

        for (label, fn_count, line_count) in &rows {
            let pct = if self.total_line_count == 0 {
                0.0
            } else {
                *line_count as f64 / self.total_line_count as f64 * 100.0
            };
            #[expect(clippy::cast_sign_loss, reason = "pct is 0..=100")]
            let bar_len = (pct / 100.0 * bar_width as f64)
                .round()
                .clamp(0.0, bar_width as f64) as usize;
            let bar: String = "\u{2588}".repeat(bar_len) + &"\u{2591}".repeat(bar_width - bar_len);
            writeln!(
                f,
                "  {:<12} {:>5} fns {:>6} lines {:>6}% |{}|",
                label,
                fn_count,
                line_count,
                format!("{pct:.1}"),
                bar
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FnEntry {
    capabilities: CapabilitySet,
    line_count: usize,
    is_test: bool,
    allows_dead_code: bool,
}

fn rvs_build_report(entries: &[FnEntry]) -> Result<Report, String> {
    let mut by_capability: BTreeMap<Capability, CapStats> = BTreeMap::new();
    let mut pure_fn_count = 0usize;
    let mut pure_line_count = 0usize;
    let mut good_fn_count = 0usize;
    let mut good_line_count = 0usize;
    let mut ok_fn_count = 0usize;
    let mut ok_line_count = 0usize;
    let mut total_fn_count = 0usize;
    let mut total_line_count = 0usize;
    for func in entries {
        if func.is_test || func.allows_dead_code {
            continue;
        }
        if func.line_count == 0 {
            return Err("report entry line count must be positive".into());
        }
        total_fn_count = rvs_checked_report_sum(total_fn_count, 1, "total function count")?;
        total_line_count =
            rvs_checked_report_sum(total_line_count, func.line_count, "total report line count")?;

        if func.capabilities.rvs_is_empty() {
            pure_fn_count = rvs_checked_report_sum(pure_fn_count, 1, "pure function count")?;
            pure_line_count =
                rvs_checked_report_sum(pure_line_count, func.line_count, "pure report line count")?;
        } else {
            for cap in func.capabilities.rvs_iter() {
                let stats = by_capability.entry(cap).or_default();
                stats.fn_count =
                    rvs_checked_report_sum(stats.fn_count, 1, "capability function count")?;
                stats.line_count = rvs_checked_report_sum(
                    stats.line_count,
                    func.line_count,
                    "capability report line count",
                )?;
            }
        }

        if CapabilityPolicy::rvs_is_good(&func.capabilities) {
            good_fn_count = rvs_checked_report_sum(good_fn_count, 1, "good function count")?;
            good_line_count =
                rvs_checked_report_sum(good_line_count, func.line_count, "good report line count")?;
        }

        if CapabilityPolicy::rvs_is_ok(&func.capabilities) {
            ok_fn_count = rvs_checked_report_sum(ok_fn_count, 1, "ok function count")?;
            ok_line_count =
                rvs_checked_report_sum(ok_line_count, func.line_count, "ok report line count")?;
        }
    }

    Ok(Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        ok_fn_count,
        ok_line_count,
        total_fn_count,
        total_line_count,
    })
}

fn rvs_checked_report_sum(current: usize, delta: usize, label: &str) -> Result<usize, String> {
    debug_assert!(current.checked_add(0).is_some(), "current count is valid");
    debug_assert!(delta.checked_add(0).is_some(), "delta count is valid");
    current
        .checked_add(delta)
        .ok_or_else(|| format!("{label} overflow while building capability report"))
}

fn rvs_report_entries_from_callgraph(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<Vec<FnEntry>, String> {
    let mut entries = Vec::new();
    let scope = LocalScope::rvs_new(local_crate_names);
    for (def_path, node) in graph.rvs_iter() {
        if !FunctionClassification::rvs_new(&scope, def_path, node).rvs_is_report_candidate() {
            continue;
        }
        let parsed = ParsedFunctionName::rvs_parse(def_path.rvs_as_str());
        let capabilities = if node.facts.is_port_method {
            CapabilityPolicy::rvs_port_method_caps()
        } else if parsed.rvs_has_rvs_prefix() {
            parsed.rvs_known_caps().clone()
        } else {
            continue;
        };
        let Some(line_count) = node.report_line_count else {
            continue;
        };
        if line_count == 0 {
            return Err(format!(
                "callgraph report line count for {def_path} must be positive"
            ));
        }
        entries.push(FnEntry {
            capabilities,
            line_count,
            is_test: node.is_test,
            allows_dead_code: node.allows_dead_code,
        });
    }
    Ok(entries)
}

fn rvs_format_contract_mismatch_summary(
    counts: &BTreeMap<FnContractMismatchKind, usize>,
    items: &[FnContractMismatch],
) -> String {
    if counts.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nContract Mismatches\n------------------------------\n");
    for (kind, count) in counts {
        out.push_str(&format!("{:<24} {}\n", kind.rvs_as_str(), count));
    }
    out.push_str("\nSample Mismatches\n------------------------------\n");
    for item in items.iter().take(10) {
        out.push_str(&format!("{}: {}\n", item.kind.rvs_as_str(), item.def_path));
    }
    out
}

fn rvs_collect_reportable_contract_diffs(
    graph: &FnGraph,
    diffs: &[FnContractDiff],
    local_crate_names: &std::collections::BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    rvs_collect_enforced_contract_diffs(graph, diffs, local_crate_names)
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_report_BIMPS(path: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;
    let local_crate_names = rvs_load_local_crate_prefixes_BIS(path)?;
    let (mut callgraph, caps) =
        rvs_collect_callgraph_and_caps_BIMS(path, true, Some(&local_crate_names))?;
    let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
    let report_entries = rvs_report_entries_from_callgraph(&callgraph, &local_crate_names)?;
    let report = rvs_build_report(&report_entries)?;
    let reportable_diffs =
        rvs_collect_reportable_contract_diffs(&callgraph, &analysis.diffs, &local_crate_names);
    let mismatch_items = rvs_collect_contract_mismatch_items(&reportable_diffs);
    let mismatch_summary = rvs_summarize_contract_mismatch_items(&mismatch_items);
    let mismatch_output = rvs_format_contract_mismatch_summary(&mismatch_summary, &mismatch_items);
    print!("{report}");
    if !mismatch_output.is_empty() {
        print!("{mismatch_output}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FnNode;
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    #[test]
    fn test_20260709_build_report_table() {
        let cases = [
            ("empty", vec![], (0usize, 0usize, 0usize, 0usize, 0usize)),
            (
                "pure_only",
                vec![FnEntry {
                    capabilities: CapabilitySet::rvs_new(),
                    line_count: 10,
                    is_test: false,
                    allows_dead_code: false,
                }],
                (1, 1, 1, 1, 10),
            ),
            (
                "mixed",
                vec![
                    FnEntry {
                        capabilities: CapabilitySet::rvs_new(),
                        line_count: 100,
                        is_test: false,
                        allows_dead_code: false,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_from_validated("BI"),
                        line_count: 50,
                        is_test: false,
                        allows_dead_code: false,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_from_validated("M"),
                        line_count: 30,
                        is_test: false,
                        allows_dead_code: false,
                    },
                ],
                (3, 1, 2, 2, 180),
            ),
            (
                "skips_test_and_dead_code",
                vec![
                    FnEntry {
                        capabilities: CapabilitySet::rvs_new(),
                        line_count: 10,
                        is_test: false,
                        allows_dead_code: false,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_new(),
                        line_count: 20,
                        is_test: true,
                        allows_dead_code: false,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_new(),
                        line_count: 30,
                        is_test: false,
                        allows_dead_code: true,
                    },
                ],
                (1, 1, 1, 1, 10),
            ),
        ];
        let mut output = String::new();
        for (name, entries, expected) in cases {
            let report = rvs_build_report(&entries).unwrap();
            output.push_str(&format!("{name}: {}\n", report.to_string().trim_end()));
            assert_eq!(
                (
                    report.total_fn_count,
                    report.pure_fn_count,
                    report.good_fn_count,
                    report.ok_fn_count,
                    report.total_line_count,
                ),
                expected,
                "{name}"
            );
        }
        rvs_snapshot_BIS("test_20260709_build_report_table", &output);
    }

    #[test]
    fn test_20260702_report_rejects_non_cargo_dir() {
        let dir = rvs_make_temp_dir_BIS("report-non-cargo");
        let result = rvs_run_report_BIMPS(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260702_report_rejects_non_cargo_dir", &output);
        assert!(result.is_err(), "report should fail for non-cargo dir");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260703_format_contract_mismatch_summary() {
        let counts = BTreeMap::from([
            (FnContractMismatchKind::MissingPort, 2usize),
            (FnContractMismatchKind::NameMismatch, 1usize),
        ]);
        let items = vec![
            FnContractMismatch {
                def_path: crate::symbols::DefPath::from("demo::rvs_fetch_BI"),
                actual_name: crate::symbols::FnName::from("rvs_fetch_BI"),
                kind: FnContractMismatchKind::MissingPort,
            },
            FnContractMismatch {
                def_path: crate::symbols::DefPath::from("demo::rvs_fetch_BI"),
                actual_name: crate::symbols::FnName::from("rvs_fetch_BI"),
                kind: FnContractMismatchKind::NameMismatch,
            },
        ];
        let output = rvs_format_contract_mismatch_summary(&counts, &items);
        rvs_snapshot_BIS("test_20260703_format_contract_mismatch_summary", &output);

        assert!(output.contains("Contract Mismatches"));
        assert!(output.contains("Sample Mismatches"));
        assert!(output.contains("missing_port"));
        assert!(output.contains("name_mismatch"));
    }

    #[test]
    fn test_20260703_collect_reportable_contract_diffs_keeps_nested_main() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::main"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::cli::main"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::User::new"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::go"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::wblk"), FnNode::default());
        graph.rvs_insert_M(
            DefPath::from("demo::Widget::sync@demo::Syncer"),
            FnNode {
                is_trait_impl: true,
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(DefPath::from("demo::parse"), FnNode::default());

        let diffs = vec![
            FnContractDiff {
                def_path: DefPath::from("demo::main"),
                actual_name: crate::symbols::FnName::from("main"),
                expected_name: Some(crate::symbols::FnName::from("rvs_main_BI")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::cli::main"),
                actual_name: crate::symbols::FnName::from("main"),
                expected_name: Some(crate::symbols::FnName::from("rvs_main")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::User::new"),
                actual_name: crate::symbols::FnName::from("new"),
                expected_name: Some(crate::symbols::FnName::from("rvs_new")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::go"),
                actual_name: crate::symbols::FnName::from("go"),
                expected_name: Some(crate::symbols::FnName::from("rvs_go")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::wblk"),
                actual_name: crate::symbols::FnName::from("wblk"),
                expected_name: Some(crate::symbols::FnName::from("rvs_wblk")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::Widget::sync@demo::Syncer"),
                actual_name: crate::symbols::FnName::from("rvs_sync_BI"),
                expected_name: Some(crate::symbols::FnName::from("rvs_sync_P")),
                declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
                expected_public_caps: Some(CapabilitySet::rvs_from_validated("P")),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::parse"),
                actual_name: crate::symbols::FnName::from("parse"),
                expected_name: Some(crate::symbols::FnName::from("rvs_parse")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
        ];

        let filtered = rvs_collect_reportable_contract_diffs(
            &graph,
            &diffs,
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        );
        let output = format!(
            "filtered={filtered:?}\nsummary={:?}\n",
            rvs_summarize_contract_mismatch_items(&rvs_collect_contract_mismatch_items(&filtered))
        );
        rvs_snapshot_BIS(
            "test_20260703_collect_reportable_contract_diffs_keeps_nested_main",
            &output,
        );

        assert_eq!(filtered.len(), 5);
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::cli::main")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::User::new")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::go")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::wblk")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::parse")
        );
    }

    #[test]
    fn test_20260709_report_entries_skip_non_port_trait_impl_methods() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Worker::rvs_run_A@demo::Runnable"),
            FnNode {
                is_trait_impl: true,
                report_line_count: Some(10),
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_plain_B"),
            FnNode {
                report_line_count: Some(3),
                ..FnNode::default()
            },
        );
        let mut port_impl = FnNode {
            is_trait_impl: true,
            report_line_count: Some(4),
            ..FnNode::default()
        };
        port_impl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::Repo::rvs_get@demo::Client"), port_impl);

        let entries = rvs_report_entries_from_callgraph(
            &graph,
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let output = format!("{entries:?}\n");
        rvs_snapshot_BIS(
            "test_20260709_report_entries_skip_non_port_trait_impl_methods",
            &output,
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| entry.capabilities == CapabilitySet::rvs_from_validated("B"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.capabilities == CapabilitySet::rvs_from_validated("P"))
        );
        assert!(
            !entries
                .iter()
                .any(|entry| entry.capabilities == CapabilitySet::rvs_from_validated("A"))
        );
    }

    #[test]
    fn test_20260710_report_entries_derive_caps_from_names_and_port_facts() {
        let mut graph = FnGraph::rvs_new();
        for (path, line_count) in [
            ("demo::rvs_mixed_AEIS", 5),
            ("demo::rvs_unknown_E", 6),
            ("demo::plain_BI", 7),
        ] {
            graph.rvs_insert_M(
                DefPath::from(path),
                FnNode {
                    report_line_count: Some(line_count),
                    ..FnNode::default()
                },
            );
        }
        let mut port = FnNode {
            report_line_count: Some(8),
            ..FnNode::default()
        };
        port.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::Repo::plain_AB@demo::Client"), port);

        let entries = rvs_report_entries_from_callgraph(
            &graph,
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let output = format!("{entries:?}\n");
        rvs_snapshot_BIS(
            "test_20260710_report_entries_derive_caps_from_names_and_port_facts",
            &output,
        );

        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|entry| {
            entry.line_count == 5 && entry.capabilities == CapabilitySet::rvs_from_validated("AIS")
        }));
        assert!(entries.iter().any(|entry| {
            entry.line_count == 6 && entry.capabilities == CapabilitySet::rvs_new()
        }));
        assert!(entries.iter().any(|entry| {
            entry.line_count == 8 && entry.capabilities == CapabilitySet::rvs_from_validated("P")
        }));
        assert!(!entries.iter().any(|entry| entry.line_count == 7));
    }

    #[test]
    fn test_20260709_report_error_table() {
        let build_zero = rvs_build_report(&[FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            line_count: 0,
            is_test: false,
            allows_dead_code: false,
        }]);
        let build_overflow = rvs_build_report(&[
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: usize::MAX,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 1,
                is_test: false,
                allows_dead_code: false,
            },
        ]);
        let sum_ok = rvs_checked_report_sum(2, 3, "demo");
        let sum_overflow = rvs_checked_report_sum(usize::MAX, 1, "demo");
        let output = format!(
            "build_zero={build_zero:?}\nbuild_overflow={build_overflow:?}\nsum_ok={sum_ok:?}\nsum_overflow={sum_overflow:?}\n"
        );
        rvs_snapshot_BIS("test_20260709_report_error_table", &output);

        assert!(build_zero.is_err());
        assert!(build_overflow.is_err());
        assert_eq!(sum_ok, Ok(5));
        assert!(sum_overflow.is_err());
    }

    #[test]
    fn test_20260708_run_report_accepts_async_fn_project() {
        let dir = rvs_make_temp_dir_BIS("report-async-project");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"report-async-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            concat!(
                "#![allow(non_snake_case)]\n",
                "pub async fn rvs_send_messages_AS(x: i32) -> i32 {\n",
                "    debug_assert!(x > 0);\n",
                "    if x > 1 { x } else { x + 1 }\n",
                "}\n",
                "\n",
                "#[cfg(test)]\n",
                "mod tests {\n",
                "    use super::*;\n",
                "\n",
                "    #[test]\n",
                "    fn test_20260708_async_report_project() {\n",
                "        std::mem::drop(rvs_send_messages_AS(1));\n",
                "    }\n",
                "}\n",
            ),
        )
        .unwrap();

        let result = rvs_run_report_BIMPS(&dir);
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIMS(&dir, true, Some(&local_crate_names)).unwrap();
        let _diffs = crate::inference::rvs_collect_local_contract_diffs_M(
            &mut callgraph,
            &caps,
            &local_crate_names,
        );
        let report_entries = rvs_report_entries_from_callgraph(&callgraph, &local_crate_names);
        let async_lines = report_entries.as_ref().ok().and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.capabilities == CapabilitySet::rvs_from_str("AS").unwrap())
                .map(|entry| entry.line_count)
        });
        let output = format!(
            "result={result:?}\nreport_entries={report_entries:?}\nasync_lines={async_lines:?}\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260708_run_report_accepts_async_fn_project", &output);

        assert!(result.is_ok(), "{output}");
        let entries = report_entries.expect("never: successful report run should produce entries");
        assert!(entries.iter().all(|entry| entry.line_count > 0), "{output}");
        assert_eq!(async_lines, Some(2), "{output}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
