use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use super::cargo_targets::CargoTargetScope;
use super::workspace::{
    rvs_canonical_cargo_project_BIS, rvs_collect_callgraph_and_caps_BIST,
    rvs_load_local_crate_prefixes_BIS,
};
use crate::artifacts::{FnGraph, FnNode};
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet, ParsedFunctionName};
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::inference::{
    FnContractMismatch, FnContractMismatchKind, PreparedLocalAnalysis,
    rvs_collect_contract_mismatch_items, rvs_summarize_contract_mismatch_items,
};
use crate::symbols::{CrateName, DefPath};

#[derive(Debug, Clone, Default)]
struct CapStats {
    fn_count: usize,
    line_count: usize,
}

impl CapStats {
    fn rvs_add_M(&mut self, entry: &FnEntry, label: &str) -> Result<(), String> {
        self.fn_count = rvs_checked_report_sum(
            self.fn_count,
            entry.function_count,
            &format!("{label} function count"),
        )?;
        self.line_count = rvs_checked_report_sum(
            self.line_count,
            entry.line_count,
            &format!("{label} report line count"),
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct Report {
    by_capability: BTreeMap<Capability, CapStats>,
    pure: CapStats,
    good: CapStats,
    ok: CapStats,
    total: CapStats,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Capability Report")?;
        writeln!(f, "{:-<60}", "")?;
        writeln!(
            f,
            "Total: {} functions, {} lines",
            self.total.fn_count, self.total.line_count
        )?;
        writeln!(f, "{:-<60}", "")?;

        if self.total.fn_count == 0 {
            writeln!(f, "(no rvs_ functions found)")?;
            return Ok(());
        }

        let bar_width = 30;
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        rows.push(("(ok)".to_string(), self.ok.fn_count, self.ok.line_count));
        rows.push((
            "(good)".to_string(),
            self.good.fn_count,
            self.good.line_count,
        ));
        rows.push((
            "(pure)".to_string(),
            self.pure.fn_count,
            self.pure.line_count,
        ));

        for cap in [
            Capability::A,
            Capability::B,
            Capability::C,
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
            let pct = if self.total.line_count == 0 {
                0.0
            } else {
                *line_count as f64 / self.total.line_count as f64 * 100.0
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
    function_count: usize,
    line_count: usize,
}

fn rvs_build_report(entries: &[FnEntry]) -> Result<Report, String> {
    let mut report = Report::default();
    for entry in entries {
        if entry.line_count == 0 {
            return Err("report entry line count must be positive".into());
        }
        debug_assert!(entry.function_count > 0, "report functions are non-empty");
        report.total.rvs_add_M(entry, "total")?;

        if entry.capabilities.rvs_is_empty() {
            report.pure.rvs_add_M(entry, "pure")?;
        } else {
            for cap in entry.capabilities.rvs_iter() {
                report
                    .by_capability
                    .entry(cap)
                    .or_default()
                    .rvs_add_M(entry, "capability")?;
            }
        }

        if CapabilityPolicy::rvs_is_good(&entry.capabilities) {
            report.good.rvs_add_M(entry, "good")?;
        }

        if CapabilityPolicy::rvs_is_ok(&entry.capabilities) {
            report.ok.rvs_add_M(entry, "ok")?;
        }
    }

    Ok(report)
}

fn rvs_checked_report_sum(current: usize, delta: usize, label: &str) -> Result<usize, String> {
    super::rvs_checked_count_sum(current, delta, label)
}

/// Whether a node contributes to the report at all (scope, test, and
/// naming-prefix gates shared by capability rows and incomplete counts).
fn rvs_is_report_function(scope: &LocalScope, def_path: &DefPath, node: &FnNode) -> bool {
    if !FunctionClassification::rvs_new(scope, def_path, node).rvs_is_report_candidate() {
        return false;
    }
    let has_per_definition_metadata = node.report_function_count > 0;
    if node.is_test || def_path.rvs_is_in_test_module() {
        return false;
    }
    if !has_per_definition_metadata && node.allows_dead_code {
        return false;
    }
    // The rvs_ prefix gates ordinary functions; World Port impl methods
    // are reported through their structural P contract even when the impl
    // method itself carries no rvs_ prefix.
    ParsedFunctionName::rvs_parse(def_path.rvs_as_str()).rvs_has_rvs_prefix()
        || node.facts.is_port_method
}

fn rvs_report_entry(
    scope: &LocalScope,
    def_path: &DefPath,
    node: &FnNode,
    semantic_caps: &CapabilitySet,
) -> Result<Option<FnEntry>, String> {
    if !rvs_is_report_function(scope, def_path, node) {
        return Ok(None);
    }
    // Reports measure callgraph semantics only: semantic caps come from the
    // prepared inference (facts + propagation + votes + capsmap), never
    // from the name. The rvs_ prefix is still required for a function to be
    // counted, but its suffix letters are ignored here.
    let capabilities = semantic_caps.clone();
    let Some(line_count) = node.report_line_count else {
        return Ok(None);
    };
    if line_count == 0 {
        return Err(format!(
            "callgraph report line count for {def_path} must be positive"
        ));
    }
    Ok(Some(FnEntry {
        capabilities,
        function_count: node.report_function_count.max(1),
        line_count,
    }))
}

fn rvs_report_entries_from_callgraph(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
    semantic_caps: &BTreeMap<DefPath, CapabilitySet>,
) -> Result<Vec<FnEntry>, String> {
    let mut entries = Vec::new();
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    for (def_path, node) in graph.rvs_iter() {
        let Some(caps) = semantic_caps.get(def_path) else {
            continue;
        };
        if let Some(entry) = rvs_report_entry(&scope, def_path, node, caps)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn rvs_incomplete_report_function_count(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
    incomplete_paths: &BTreeSet<DefPath>,
) -> Result<usize, String> {
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let mut count = 0usize;
    for path in incomplete_paths {
        let Some(node) = graph.rvs_get(path.rvs_as_str()) else {
            continue;
        };
        if !rvs_is_report_function(&scope, path, node) || node.report_line_count.is_none() {
            continue;
        }
        let function_count = node.report_function_count.max(1);
        count = rvs_checked_report_sum(count, function_count, "incomplete report function count")?;
    }
    Ok(count)
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

fn rvs_format_incomplete_inference_summary(count: usize) -> String {
    if count == 0 {
        debug_assert_eq!(count, 0);
        return String::new();
    }
    debug_assert!(count > 0);
    format!(
        "\nInference Status\n------------------------------\n{count} local function(s) depend on unknown callee capability data. Capability totals reflect measured signature/body facts and callgraph propagation from known caps only; rename suggestions may omit unknown capabilities.\n"
    )
}

fn rvs_format_trait_outlier_summary(
    outliers: &[crate::offline_caps::TargetTraitImplOutlierGroup],
) -> String {
    if outliers.is_empty() {
        return String::new();
    }
    let mut output = format!(
        "\nTrait Vote Outliers\n------------------------------\n{} target-specific local trait implementation group(s) have propagated capabilities outside their aggregate vote. Capability totals are unchanged.\n",
        outliers.len()
    );
    for group in outliers.iter().take(10) {
        let outlier = &group.outlier;
        let targets = group
            .crate_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        output.push_str(&format!(
            "{}: {} outside {} for {} (threshold {}/{}, targets {targets})\n",
            outlier.implementation,
            outlier.unexpected_caps.rvs_letters(),
            outlier.selected_caps.rvs_letters_or_pure(),
            outlier.trait_method,
            outlier.threshold,
            outlier.implementations,
        ));
    }
    output
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_report_BIPST(path: &Path) -> Result<(), String> {
    let project_path = rvs_canonical_cargo_project_BIS(path)?;
    let target_scope = CargoTargetScope::WithTestExampleBench;
    let local_crate_names = rvs_load_local_crate_prefixes_BIS(&project_path, target_scope)?;
    let (mut callgraph, caps) =
        rvs_collect_callgraph_and_caps_BIST(&project_path, target_scope, &local_crate_names)?;
    let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
    let target_outliers = crate::offline_caps::rvs_collect_report_trait_impl_outliers(
        &callgraph,
        &local_crate_names,
        &analysis,
    );
    let report_entries =
        rvs_report_entries_from_callgraph(&callgraph, &local_crate_names, analysis.rvs_inferred())?;
    let report = rvs_build_report(&report_entries)?;
    let incomplete_count = rvs_incomplete_report_function_count(
        &callgraph,
        &local_crate_names,
        analysis.rvs_incomplete_paths(),
    )?;
    let incomplete_output = rvs_format_incomplete_inference_summary(incomplete_count);
    let outlier_output = rvs_format_trait_outlier_summary(&target_outliers);
    let mismatch_items = rvs_collect_contract_mismatch_items(&analysis.diffs);
    let mismatch_summary = rvs_summarize_contract_mismatch_items(&mismatch_items);
    let mismatch_output = rvs_format_contract_mismatch_summary(&mismatch_summary, &mismatch_items);
    print!("{report}");
    if !incomplete_output.is_empty() {
        print!("{incomplete_output}");
    }
    if !outlier_output.is_empty() {
        print!("{outlier_output}");
    }
    if !mismatch_output.is_empty() {
        print!("{mismatch_output}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallEdgeType, FnNode, FnSource, FunctionIdentity};
    use crate::symbols::DefPath;
    use crate::test_support::{
        rvs_make_capsmap, rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };

    fn rvs_report_target_node(crate_id: u64, calls: &[&str], is_trait_impl: bool) -> FnNode {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        let mut node = FnNode::default();
        node.calls = calls
            .iter()
            .map(|call| {
                (
                    FunctionIdentity {
                        crate_id: 900,
                        def_path: DefPath::from(*call),
                    },
                    CallEdgeType::Strong,
                )
            })
            .collect();
        node.is_trait_impl = is_trait_impl;
        node.is_production = true;
        node.is_coverage_candidate = true;
        node.crate_id = crate_id;
        node.sources
            .insert(FnSource::rvs_new("src/lib.rs".into(), 1, 2));
        node
    }

    #[test]
    fn test_20260715_report_describes_incomplete_inference_without_false_bounds() {
        let output = format!(
            "zero={:?}\nnonzero={}",
            rvs_format_incomplete_inference_summary(0),
            rvs_format_incomplete_inference_summary(3)
        );
        rvs_snapshot_BIS(
            "test_20260715_report_describes_incomplete_inference_without_false_bounds",
            &output,
        );

        assert!(output.contains("3 local function(s)"));
        assert!(output.contains(
            "Capability totals reflect measured signature/body facts and callgraph propagation from known caps only"
        ));
        assert!(output.contains("rename suggestions may omit unknown capabilities"));
        assert!(!output.contains("lower bounds"));
    }

    #[test]
    fn test_20260715_report_formats_trait_vote_outliers_without_changing_totals() {
        let outlier = crate::inference::TraitImplOutlier {
            trait_method: DefPath::from("demo::FromString::rvs_parse"),
            implementation: DefPath::from("demo::EnvValue::rvs_parse@demo::FromString"),
            implementation_caps: CapabilitySet::rvs_from_validated("S"),
            selected_caps: CapabilitySet::rvs_new(),
            unexpected_caps: CapabilitySet::rvs_from_validated("S"),
            implementations: 3,
            threshold: 2,
            counts: BTreeMap::from([(Capability::S, 1)]),
        };
        let group = crate::offline_caps::TargetTraitImplOutlierGroup {
            outlier,
            crate_ids: BTreeSet::from([7, 9]),
        };
        let output = format!(
            "empty={:?}\nnonempty={}",
            rvs_format_trait_outlier_summary(&[]),
            rvs_format_trait_outlier_summary(&[group]),
        );
        rvs_snapshot_BIS(
            "test_20260715_report_formats_trait_vote_outliers_without_changing_totals",
            &output,
        );

        assert!(output.contains("Capability totals are unchanged"));
        assert!(output.contains("S outside pure"));
        assert!(output.contains("targets 7,9"));
    }

    #[test]
    fn test_20260716_report_uses_cross_crate_target_trait_vote() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_report_target_node(100, &[], false);
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::Parser::rvs_parse"), declaration);
        graph.rvs_insert_M(
            DefPath::from("demo::Alpha::rvs_parse@demo::Parser"),
            rvs_report_target_node(11, &[], true),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Beta::rvs_parse@demo::Parser"),
            rvs_report_target_node(12, &[], true),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Gamma::rvs_parse@demo::Parser"),
            rvs_report_target_node(13, &["dependency::effect"], true),
        );
        let local = BTreeSet::from([CrateName::from("demo")]);
        let caps = rvs_make_capsmap(&[("dependency::effect", "S")]);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut graph, &caps, &local);
        let outliers =
            crate::offline_caps::rvs_collect_report_trait_impl_outliers(&graph, &local, &analysis);
        let output = rvs_format_trait_outlier_summary(&outliers);
        rvs_snapshot_BIS(
            "test_20260716_report_uses_cross_crate_target_trait_vote",
            &output,
        );

        assert_eq!(outliers.len(), 1);
        assert!(output.contains("demo::Gamma::rvs_parse@demo::Parser"));
        assert!(output.contains("threshold 2/3"));
    }

    #[test]
    fn test_20260715_report_incomplete_count_matches_entry_eligibility_and_multiplicity() {
        let mut graph = FnGraph::rvs_new();
        let rvs_incomplete_node = |line_count, function_count| {
            let mut node = FnNode {
                report_line_count: line_count,
                report_function_count: function_count,
                ..FnNode::default()
            };
            node.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("dep::unknown"),
                },
                CallEdgeType::Strong,
            );
            node
        };

        graph.rvs_insert_M(
            DefPath::from("demo::rvs_included"),
            rvs_incomplete_node(Some(5), 3),
        );

        let mut test = rvs_incomplete_node(Some(2), 1);
        test.is_test = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_test"), test);

        let mut test_compilation = rvs_incomplete_node(Some(2), 1);
        test_compilation.is_test_compilation = true;
        graph.rvs_insert_M(
            DefPath::from("demo::tests::rvs_test_compilation"),
            test_compilation,
        );

        let mut skipped_dead_code = rvs_incomplete_node(Some(2), 0);
        skipped_dead_code.allows_dead_code = true;
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_skipped_dead_code"),
            skipped_dead_code,
        );

        graph.rvs_insert_M(
            DefPath::from("demo::plain"),
            rvs_incomplete_node(Some(2), 1),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_missing_line_count"),
            rvs_incomplete_node(None, 1),
        );

        let mut trait_impl = rvs_incomplete_node(Some(2), 1);
        trait_impl.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Worker::rvs_run@demo::Runnable"),
            trait_impl,
        );

        let local = BTreeSet::from([CrateName::from("demo")]);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &local,
        );
        let entries =
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred()).unwrap();
        let report = rvs_build_report(&entries).unwrap();
        let incomplete_count =
            rvs_incomplete_report_function_count(&graph, &local, analysis.rvs_incomplete_paths())
                .unwrap();
        let output = format!(
            "all_incomplete_paths={}\nentries={}\nreported_functions={}\nincomplete_report_functions={}\n",
            analysis.rvs_incomplete_paths().len(),
            entries.len(),
            report.total.fn_count,
            incomplete_count,
        );
        rvs_snapshot_BIS(
            "test_20260715_report_incomplete_count_matches_entry_eligibility_and_multiplicity",
            &output,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(report.total.fn_count, 3);
        assert_eq!(incomplete_count, 3);
    }

    #[test]
    fn test_20260709_build_report_table() {
        let cases = [
            ("empty", vec![], (0usize, 0usize, 0usize, 0usize, 0usize)),
            (
                "pure_only",
                vec![FnEntry {
                    capabilities: CapabilitySet::rvs_new(),
                    function_count: 1,
                    line_count: 10,
                }],
                (1, 1, 1, 1, 10),
            ),
            (
                "mixed",
                vec![
                    FnEntry {
                        capabilities: CapabilitySet::rvs_new(),
                        function_count: 1,
                        line_count: 100,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_from_validated("BI"),
                        function_count: 1,
                        line_count: 50,
                    },
                    FnEntry {
                        capabilities: CapabilitySet::rvs_from_validated("M"),
                        function_count: 1,
                        line_count: 30,
                    },
                ],
                (3, 1, 2, 2, 180),
            ),
            (
                "aggregated_targets",
                vec![FnEntry {
                    capabilities: CapabilitySet::rvs_new(),
                    function_count: 2,
                    line_count: 25,
                }],
                (2, 2, 2, 2, 25),
            ),
            (
                // C is a measured non-suffix capability: it renders its own
                // row and still counts as good (subset of ABCM).
                "const_only",
                vec![FnEntry {
                    capabilities: CapabilitySet::rvs_from_validated("C"),
                    function_count: 1,
                    line_count: 5,
                }],
                (1, 0, 1, 1, 5),
            ),
        ];
        let mut output = String::new();
        for (name, entries, expected) in cases {
            let report = rvs_build_report(&entries).unwrap();
            output.push_str(&format!("{name}: {}\n", report.to_string().trim_end()));
            assert_eq!(
                (
                    report.total.fn_count,
                    report.pure.fn_count,
                    report.good.fn_count,
                    report.ok.fn_count,
                    report.total.line_count,
                ),
                expected,
                "{name}"
            );
        }
        rvs_snapshot_BIS("test_20260709_build_report_table", &output);
    }

    #[test]
    fn test_20260714_report_excludes_test_compilation_only_helpers() {
        let mut graph = FnGraph::rvs_new();
        let mut production = FnNode {
            report_line_count: Some(3),
            ..FnNode::default()
        };
        production
            .sources
            .insert(FnSource::rvs_new("src/lib.rs".into(), 1, 2));
        graph.rvs_insert_M(DefPath::from("demo::rvs_production"), production);

        let mut test_helper = FnNode {
            is_test_compilation: true,
            report_line_count: Some(0),
            report_function_count: 1,
            ..FnNode::default()
        };
        test_helper
            .sources
            .insert(FnSource::rvs_new("src/lib.rs".into(), 3, 4));
        graph.rvs_insert_M(DefPath::from("demo::tests::rvs_helper"), test_helper);

        let mut partially_allowed = FnNode {
            allows_dead_code: true,
            report_line_count: Some(2),
            report_function_count: 1,
            ..FnNode::default()
        };
        partially_allowed
            .sources
            .insert(FnSource::rvs_new("src/main.rs".into(), 5, 6));
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_partially_allowed"),
            partially_allowed,
        );

        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut scoped_graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &local,
        );
        let entries =
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred()).unwrap();
        let report = rvs_build_report(&entries).unwrap();
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260714_report_excludes_test_compilation_only_helpers",
            &output,
        );

        assert_eq!(report.total.fn_count, 2);
        assert_eq!(report.total.line_count, 5);
    }

    #[test]
    fn test_20260702_report_rejects_non_cargo_dir() {
        let dir = rvs_make_temp_dir_BIS("report-non-cargo");
        let result = rvs_run_report_BIPST(&dir);
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
        let rvs_sourced_node = || FnNode {
            sources: std::collections::BTreeSet::from([FnSource::rvs_new(
                std::path::PathBuf::from("src/lib.rs"),
                1,
                2,
            )]),
            ..FnNode::default()
        };
        graph.rvs_insert_M(
            DefPath::from("demo::main"),
            FnNode {
                is_entrypoint: true,
                ..rvs_sourced_node()
            },
        );
        graph.rvs_insert_M(DefPath::from("demo::cli::main"), rvs_sourced_node());
        graph.rvs_insert_M(DefPath::from("demo::User::new"), rvs_sourced_node());
        graph.rvs_insert_M(DefPath::from("demo::go"), rvs_sourced_node());
        graph.rvs_insert_M(DefPath::from("demo::wblk"), rvs_sourced_node());
        graph.rvs_insert_M(
            DefPath::from("demo::Widget::sync@demo::Syncer"),
            FnNode {
                is_trait_impl: true,
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(DefPath::from("demo::parse"), rvs_sourced_node());

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        );
        let filtered = analysis.diffs;
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
    fn test_20260713_report_enforces_library_crate_root_main() {
        let dir = rvs_make_cargo_project_BIS(
            "report-library-root-main",
            "report-library-main",
            &[("src/lib.rs", "pub fn main() -> i32 { 1 }\n")],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let def_path = "report_library_main::main";
        let diff = analysis
            .diffs
            .iter()
            .find(|diff| diff.def_path.rvs_as_str() == def_path);
        let mismatch_kinds = diff.map(|diff| diff.rvs_mismatch_kinds());
        let output = format!(
            "node={}\ndiff={}\nexpected={:?}\nmismatches={mismatch_kinds:?}\n",
            callgraph.rvs_get(def_path).is_some(),
            diff.is_some(),
            diff.map(|diff| diff.expected_name.rvs_as_str()),
        );
        rvs_snapshot_BIS(
            "test_20260713_report_enforces_library_crate_root_main",
            &output,
        );

        let diff = diff;
        assert!(
            diff.is_none(),
            "main is an entrypoint; no contract diff expected"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_preserves_binary_entry_across_all_targets() {
        let dir = rvs_make_cargo_project_BIS(
            "report-binary-entry-all-targets",
            "report-binary-entry",
            &[("src/main.rs", "fn main() {}\n")],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let def_path = "report_binary_entry::main";
        let node = callgraph
            .rvs_get(def_path)
            .expect("binary entry should be collected");
        let source_files: Vec<_> = node
            .sources
            .iter()
            .filter_map(|source| source.file.file_name())
            .collect();
        let has_diff = analysis
            .diffs
            .iter()
            .any(|diff| diff.def_path.rvs_as_str() == def_path);
        let output = format!(
            "entry={}\nsource_files={source_files:?}\ncontract_diff={has_diff}\n",
            node.is_entrypoint,
        );
        rvs_snapshot_BIS(
            "test_20260713_report_preserves_binary_entry_across_all_targets",
            &output,
        );

        assert!(node.is_entrypoint);
        assert!(!has_diff);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_enforces_integration_test_main_as_ordinary() {
        let dir = rvs_make_cargo_project_BIS(
            "report-integration-test-main",
            "report-integration-main",
            &[
                ("src/lib.rs", "pub fn rvs_value() -> i32 { 1 }\n"),
                ("tests/helper.rs", "fn main() -> i32 { 2 }\n"),
            ],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let def_path = "helper::main";
        let node = callgraph
            .rvs_get(def_path)
            .expect("integration-test main should be collected");
        let diff = analysis
            .diffs
            .iter()
            .find(|diff| diff.def_path.rvs_as_str() == def_path);
        let output = format!(
            "entry={}\nexpected={:?}\n",
            node.is_entrypoint,
            diff.map(|diff| diff.expected_name.rvs_as_str()),
        );
        rvs_snapshot_BIS(
            "test_20260713_report_enforces_integration_test_main_as_ordinary",
            &output,
        );

        assert!(node.is_entrypoint);
        assert_eq!(diff.map(|diff| diff.expected_name.rvs_as_str()), None);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_enforces_no_main_binary_function() {
        let dir = rvs_make_cargo_project_BIS(
            "report-no-main-binary-function",
            "report-no-main-binary",
            &[("src/main.rs", "#![no_main]\nfn main() -> i32 { 1 }\n")],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let collected = rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names);
        let output = match collected {
            Ok((mut callgraph, caps)) => {
                let analysis =
                    PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
                let def_path = "report_no_main_binary::main";
                let node = callgraph
                    .rvs_get(def_path)
                    .expect("no_main function should be collected");
                let expected = analysis
                    .diffs
                    .iter()
                    .find(|diff| diff.def_path.rvs_as_str() == def_path)
                    .map(|diff| diff.expected_name.rvs_as_str());
                assert!(node.is_entrypoint);
                assert_eq!(expected, None);
                format!("entry={}\nexpected={expected:?}\n", node.is_entrypoint)
            }
            Err(error) => panic!("no_main binary collection should succeed: {error}"),
        };
        rvs_snapshot_BIS(
            "test_20260713_report_enforces_no_main_binary_function",
            &output,
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_enforces_cfg_test_only_main() {
        let dir = rvs_make_cargo_project_BIS(
            "report-cfg-test-only-main",
            "report-cfg-main",
            &[(
                "src/main.rs",
                "#[cfg(not(test))]\nfn main() {}\n\n#[cfg(test)]\nfn main(value: i32) -> i32 { value }\n",
            )],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let def_path = "report_cfg_main::main";
        let node = callgraph
            .rvs_get(def_path)
            .expect("cfg(test) main should be collected");
        let diff = analysis
            .diffs
            .iter()
            .find(|diff| diff.def_path.rvs_as_str() == def_path);
        let source_ranges: Vec<_> = node
            .sources
            .iter()
            .map(|source| (source.name_start, source.name_end))
            .collect();
        let output = format!(
            "entry={}\ntest_compilation={}\nsource_ranges={source_ranges:?}\nexpected={:?}\nentry_calls={:?}\n",
            node.is_entrypoint,
            node.is_test_compilation,
            diff.map(|diff| diff.expected_name.rvs_as_str()),
            node.entry_calls,
        );
        rvs_snapshot_BIS("test_20260713_report_enforces_cfg_test_only_main", &output);

        assert!(node.is_entrypoint);
        assert!(!node.is_test_compilation);
        assert_eq!(source_ranges.len(), 1);
        assert_eq!(diff.map(|diff| diff.expected_name.rvs_as_str()), None);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_same_name_lib_bin_retains_only_library_main() {
        let dir = rvs_make_cargo_project_BIS(
            "report-same-name-lib-bin-main",
            "report-same-name-main",
            &[
                ("src/lib.rs", "pub fn main() -> i32 { 1 }\n"),
                ("src/main.rs", "fn main() { std::process::exit(0); }\n"),
            ],
        );
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let def_path = "report_same_name_main::main";
        let node = callgraph
            .rvs_get(def_path)
            .expect("same-name library main should be retained");
        let source_files: Vec<_> = node
            .sources
            .iter()
            .filter_map(|source| source.file.file_name())
            .collect();
        let diff = analysis
            .diffs
            .iter()
            .find(|diff| diff.def_path.rvs_as_str() == def_path);
        let entry_calls: Vec<_> = node
            .calls
            .keys()
            .map(|identity| identity.def_path.rvs_as_str().to_string())
            .chain(
                node.entry_calls
                    .keys()
                    .map(|path| path.rvs_as_str().to_string()),
            )
            .collect();
        let output = format!(
            "entry={}\nsource_files={source_files:?}\nexpected={:?}\nentry_calls={entry_calls:?}\n",
            node.is_entrypoint,
            diff.map(|diff| diff.expected_name.rvs_as_str()),
        );
        rvs_snapshot_BIS(
            "test_20260713_report_same_name_lib_bin_retains_only_library_main",
            &output,
        );

        assert!(node.is_entrypoint);
        assert_eq!(
            source_files,
            vec![
                std::ffi::OsStr::new("lib.rs"),
                std::ffi::OsStr::new("main.rs")
            ]
        );
        assert_eq!(diff.map(|diff| diff.expected_name.rvs_as_str()), None);
        assert!(entry_calls.iter().any(|path| path == "std::process::exit"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_report_skips_sourceless_generated_helpers() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::OpenFileSnafu::build"),
            FnNode::default(),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::CustomContext::fail"),
            FnNode::default(),
        );
        let mut user_node = FnNode::default();
        user_node.sources.insert(FnSource::rvs_new(
            std::path::PathBuf::from("src/lib.rs"),
            1,
            6,
        ));
        graph.rvs_insert_M(DefPath::from("demo::UserSnafu::build"), user_node);

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        );
        let diff_paths: Vec<_> = analysis
            .diffs
            .iter()
            .map(|diff| diff.def_path.rvs_as_str())
            .collect();
        let mismatch_items = rvs_collect_contract_mismatch_items(&analysis.diffs);
        let output = format!("diff_paths={diff_paths:?}\nmismatches={mismatch_items:?}\n");
        rvs_snapshot_BIS(
            "test_20260713_report_skips_sourceless_generated_helpers",
            &output,
        );

        assert_eq!(diff_paths, vec!["demo::UserSnafu::build"]);
        assert_eq!(mismatch_items.len(), 1);
        assert_eq!(
            mismatch_items[0].kind,
            FnContractMismatchKind::MissingRvsPrefix
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

        let local = std::collections::BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut scoped_graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &local,
        );
        let entries =
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred()).unwrap();
        let output = format!("{entries:?}\n");
        rvs_snapshot_BIS(
            "test_20260709_report_entries_skip_non_port_trait_impl_methods",
            &output,
        );

        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| entry.capabilities == CapabilitySet::rvs_new())
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
    fn test_20260816_report_entries_derive_caps_from_callgraph_facts() {
        let mut graph = FnGraph::rvs_new();
        // Name suffixes are ignored: capabilities come from facts and
        // callee edges only.
        let mut io_static = FnNode {
            report_line_count: Some(5),
            ..FnNode::default()
        };
        io_static.facts.has_static_ref = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_mixed_AEIS"), io_static);
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_unknown_E"),
            FnNode {
                report_line_count: Some(6),
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::plain_BI"),
            FnNode {
                report_line_count: Some(7),
                ..FnNode::default()
            },
        );
        let mut port = FnNode {
            report_line_count: Some(8),
            ..FnNode::default()
        };
        port.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::Repo::plain_AB@demo::Client"), port);

        let local = std::collections::BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut scoped_graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &local,
        );
        let entries =
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred()).unwrap();
        let output = format!("{entries:?}\n");
        rvs_snapshot_BIS(
            "test_20260816_report_entries_derive_caps_from_callgraph_facts",
            &output,
        );

        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|entry| {
            entry.line_count == 5 && entry.capabilities == CapabilitySet::rvs_from_validated("S")
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
    fn test_20260816_report_merges_non_suffix_caps_from_facts() {
        let mut graph = FnGraph::rvs_new();
        let mut async_fn = FnNode {
            report_line_count: Some(3),
            ..FnNode::default()
        };
        async_fn.facts.has_async = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_fetch_I"), async_fn);
        let mut const_fn = FnNode {
            report_line_count: Some(4),
            ..FnNode::default()
        };
        const_fn.facts.has_const = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_factorial"), const_fn);
        // U has two sources: `unsafe fn` signatures and `static mut` access
        // inside a safe fn.
        let mut unsafe_fn = FnNode {
            report_line_count: Some(5),
            ..FnNode::default()
        };
        unsafe_fn.facts.is_unsafe_fn = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_raw_B"), unsafe_fn);
        let mut static_mut_reader = FnNode {
            report_line_count: Some(6),
            ..FnNode::default()
        };
        static_mut_reader.facts.has_static_mut_ref = true;
        graph.rvs_insert_M(DefPath::from("demo::rvs_read_counter_S"), static_mut_reader);

        let local = std::collections::BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut scoped_graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &local,
        );
        let entries =
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred()).unwrap();
        let output = format!("{entries:?}\n");
        rvs_snapshot_BIS(
            "test_20260816_report_merges_non_suffix_caps_from_facts",
            &output,
        );

        assert_eq!(entries.len(), 4);
        assert!(entries.iter().any(|entry| {
            entry.line_count == 3 && entry.capabilities == CapabilitySet::rvs_from_validated("A")
        }));
        assert!(entries.iter().any(|entry| {
            entry.line_count == 4 && entry.capabilities == CapabilitySet::rvs_from_validated("C")
        }));
        assert!(entries.iter().any(|entry| {
            entry.line_count == 5 && entry.capabilities == CapabilitySet::rvs_from_validated("U")
        }));
        assert!(entries.iter().any(|entry| {
            entry.line_count == 6 && entry.capabilities == CapabilitySet::rvs_from_validated("SU")
        }));
    }

    #[test]
    fn test_20260816_report_is_invariant_under_renaming() {
        // Reports measure callgraph semantics; renaming (adding/removing
        // suffix letters) must only change naming diagnostics, never the
        // per-function capability rows.
        let build = |names: &[&str]| {
            let mut graph = FnGraph::rvs_new();
            let mut io_fn = FnNode {
                report_line_count: Some(5),
                ..FnNode::default()
            };
            io_fn.calls.insert(
                FunctionIdentity {
                    crate_id: 2,
                    def_path: DefPath::from("dep::fs::read"),
                },
                CallEdgeType::Strong,
            );
            graph.rvs_insert_M(DefPath::from(names[0]), io_fn);
            let mut pure_fn = FnNode {
                report_line_count: Some(3),
                ..FnNode::default()
            };
            pure_fn.facts.has_const = true;
            graph.rvs_insert_M(DefPath::from(names[1]), pure_fn);
            let local = std::collections::BTreeSet::from([CrateName::from("demo")]);
            let mut scoped_graph = graph.clone();
            let analysis = PreparedLocalAnalysis::rvs_prepare_M(
                &mut scoped_graph,
                &crate::capsmap::CapsMap::rvs_new(),
                &local,
            );
            rvs_report_entries_from_callgraph(&graph, &local, analysis.rvs_inferred())
                .unwrap()
                .into_iter()
                .map(|entry| entry.capabilities.rvs_letters())
                .collect::<Vec<_>>()
        };
        let plain = build(&["demo::rvs_handle", "demo::rvs_factorial"]);
        let forged = build(&["demo::rvs_handle_ST", "demo::rvs_factorial_M"]);
        let output = format!("plain={plain:?}\nforged={forged:?}\n");
        rvs_snapshot_BIS("test_20260816_report_is_invariant_under_renaming", &output);

        assert_eq!(plain, forged);
        // C is measured from the const fn signature; the callee edge
        // without capsmap knowledge contributes no capability (it lands in
        // the incomplete-inference summary instead).
        assert!(plain.iter().any(|caps| caps == &"C".to_string()));
    }

    #[test]
    fn test_20260709_report_error_table() {
        let build_zero = rvs_build_report(&[FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            function_count: 1,
            line_count: 0,
        }]);
        let build_overflow = rvs_build_report(&[
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                function_count: 1,
                line_count: usize::MAX,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                function_count: 1,
                line_count: 1,
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
        let dir = rvs_make_cargo_project_BIS(
            "report-async-project",
            "report-async-project",
            &[(
                "src/lib.rs",
                concat!(
                    "#![allow(non_snake_case)]\n",
                    "pub async fn rvs_send_messages_S(x: i32) -> i32 {\n",
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
                    "        std::mem::drop(rvs_send_messages_S(1));\n",
                    "    }\n",
                    "}\n",
                ),
            )],
        );

        let result = rvs_run_report_BIPST(&dir);
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope).unwrap();
        let (mut callgraph, caps) =
            rvs_collect_callgraph_and_caps_BIST(&dir, target_scope, &local_crate_names).unwrap();
        let _diffs = crate::inference::rvs_collect_local_contract_diffs_M(
            &mut callgraph,
            &caps,
            &local_crate_names,
        );
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &caps, &local_crate_names);
        let report_entries = rvs_report_entries_from_callgraph(
            &callgraph,
            &local_crate_names,
            analysis.rvs_inferred(),
        );
        let async_lines = report_entries.as_ref().ok().and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.capabilities.rvs_contains(Capability::A))
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
