use std::collections::{BTreeMap, BTreeSet, HashSet};

use rustc_lint::LateContext;

use super::ctx::{CoverageFn, TestCallTarget, TestSite};
use super::msg::rvs_emit_node_span_lint_S;
use super::{
    RVS_DUPLICATE_TEST, RVS_MISSING_TEST_OUTPUT, RVS_UNTESTED_GOOD_FN, RVS_UNTESTED_OK_FN,
};
use crate::artifacts::{FnGraph, FunctionIdentity};
use crate::lints::LintEnvironment;
use crate::symbols::CrateName;

/// `check_crate_post` — cross-cutting test quality checks and output writing.
pub(crate) fn rvs_check_crate_post_MPS<'tcx, E: LintEnvironment>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<TestSite>>,
    good_fns: &[CoverageFn],
    ok_fns: &[CoverageFn],
    test_calls: &HashSet<TestCallTarget>,
    selected_untested_functions: Option<
        &BTreeMap<FunctionIdentity, crate::artifacts::CoverageLabel>,
    >,
    callgraph: &FnGraph,
    test_outputs: Option<&BTreeSet<String>>,
    world: &mut E::World,
    check_local_coverage: bool,
    ui_testing: bool,
) {
    rvs_check_duplicate_tests_S(cx, test_names);
    rvs_check_missing_test_output_S(cx, test_names, test_outputs, ui_testing);
    if let Some(selected) = selected_untested_functions {
        rvs_check_selected_untested_fns_S(cx, good_fns, ok_fns, selected);
    } else if check_local_coverage || ui_testing {
        let covered = rvs_direct_covered_functions(callgraph);
        let mut candidate_name_counts = BTreeMap::new();
        for candidate in good_fns.iter().chain(ok_fns) {
            *candidate_name_counts
                .entry(candidate.name.clone())
                .or_insert(0usize) += 1;
        }
        rvs_check_untested_fns_S(
            cx,
            good_fns,
            ok_fns,
            test_calls,
            &covered,
            &candidate_name_counts,
        );
    }
    rvs_write_callgraph_MPS::<E>(cx, callgraph, world);
}

fn rvs_check_selected_untested_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    good_fns: &[CoverageFn],
    ok_fns: &[CoverageFn],
    selected: &BTreeMap<FunctionIdentity, crate::artifacts::CoverageLabel>,
) {
    // The selection carries the coverage class computed by the offline
    // engine from semantic caps; the emission compile only resolves spans
    // and must not reclassify from signature facts, which cannot see
    // propagated capabilities.
    for candidate in good_fns.iter().chain(ok_fns) {
        let Some(label) = selected.get(&candidate.identity) else {
            continue;
        };
        let (lint, label) = match label {
            crate::artifacts::CoverageLabel::Good => (RVS_UNTESTED_GOOD_FN, "good"),
            crate::artifacts::CoverageLabel::Ok => (RVS_UNTESTED_OK_FN, "ok"),
        };
        rvs_emit_node_span_lint_S(
            cx,
            lint,
            candidate.hir_id,
            candidate.span,
            format!("{label} fn '{}' not called by any test", candidate.name),
        );
    }
}

fn rvs_check_duplicate_tests_S<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<TestSite>>,
) {
    for (name, spans) in test_names {
        if spans.len() > 1 {
            for site in spans {
                rvs_emit_node_span_lint_S(
                    cx,
                    RVS_DUPLICATE_TEST,
                    site.hir_id,
                    site.span,
                    format!("duplicate test '{name}'"),
                );
            }
        }
    }
}

fn rvs_check_missing_test_output_S<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<TestSite>>,
    test_outputs: Option<&BTreeSet<String>>,
    ui_testing: bool,
) {
    if ui_testing {
        return;
    }
    let Some(test_outputs) = test_outputs else {
        return;
    };
    for (name, spans) in test_names {
        let out_file = format!("test_out/{name}.out");
        if !test_outputs.contains(name) {
            if let Some(site) = spans.first() {
                rvs_emit_node_span_lint_S(
                    cx,
                    RVS_MISSING_TEST_OUTPUT,
                    site.hir_id,
                    site.span,
                    format!("test '{name}' missing {out_file}"),
                );
            }
        }
    }
}

fn rvs_check_untested_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    good_fns: &[CoverageFn],
    ok_fns: &[CoverageFn],
    test_calls: &HashSet<TestCallTarget>,
    covered: &BTreeSet<FunctionIdentity>,
    candidate_name_counts: &BTreeMap<String, usize>,
) {
    for (candidates, lint, label) in [
        (good_fns, RVS_UNTESTED_GOOD_FN, "good"),
        (ok_fns, RVS_UNTESTED_OK_FN, "ok"),
    ] {
        for candidate in candidates {
            if !rvs_test_calls_function(test_calls, covered, candidate_name_counts, candidate) {
                rvs_emit_node_span_lint_S(
                    cx,
                    lint,
                    candidate.hir_id,
                    candidate.span,
                    format!("{label} fn '{}' not called by any test", candidate.name),
                );
            }
        }
    }
}

fn rvs_test_calls_function(
    test_calls: &HashSet<TestCallTarget>,
    covered: &BTreeSet<FunctionIdentity>,
    candidate_name_counts: &BTreeMap<String, usize>,
    candidate: &CoverageFn,
) -> bool {
    covered.contains(&candidate.identity)
        || test_calls.contains(&TestCallTarget::Resolved(candidate.identity.clone()))
        || (candidate_name_counts.get(&candidate.name).copied() == Some(1)
            && test_calls.contains(&TestCallTarget::UnresolvedName(candidate.name.clone())))
}

fn rvs_direct_covered_functions(graph: &FnGraph) -> BTreeSet<FunctionIdentity> {
    graph.rvs_test_reachable_identities()
}

fn rvs_write_callgraph_MPS<'tcx, E: LintEnvironment>(
    cx: &LateContext<'tcx>,
    callgraph: &FnGraph,
    world: &mut E::World,
) {
    let crate_name = CrateName::rvs_from_manifest_name(
        cx.tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).as_str(),
    );
    if let Err(error) = E::rvs_write_callgraph_BIMPST(world, &crate_name, callgraph) {
        cx.tcx
            .dcx()
            .err(format!("cannot write rivus callgraph artifact: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallEdgeType, FnNode};
    use crate::symbols::DefPath;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260714_test_call_target_matching() {
        let candidate = CoverageFn {
            identity: FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_run"),
            },
            name: "rvs_run".to_string(),
            hir_id: rustc_hir::CRATE_HIR_ID,
            span: rustc_span::DUMMY_SP,
        };
        let duplicate_candidate = CoverageFn {
            identity: FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("other::rvs_run"),
            },
            name: "rvs_run".to_string(),
            hir_id: rustc_hir::CRATE_HIR_ID,
            span: rustc_span::DUMMY_SP,
        };
        let resolved_calls = HashSet::from([TestCallTarget::Resolved(FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_run"),
        })]);
        let wrong_crate_calls = HashSet::from([TestCallTarget::Resolved(FunctionIdentity {
            crate_id: 2,
            def_path: DefPath::from("demo::rvs_run"),
        })]);
        let unresolved_calls =
            HashSet::from([TestCallTarget::UnresolvedName("rvs_run".to_string())]);
        let missing_calls = HashSet::new();
        let mut graph = FnGraph::rvs_new();
        let helper_identity = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_helper"),
        };
        let target_identity = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_target"),
        };
        let mut test_node = FnNode::default();
        let test_target = test_node.rvs_test_target_M(1);
        test_target.is_test = true;
        test_target.calls = BTreeMap::from([(helper_identity.clone(), CallEdgeType::Strong)]);
        graph.rvs_insert_M(DefPath::from("demo::test_calls_helper"), test_node);
        let mut helper_node = FnNode::default();
        helper_node.calls = BTreeMap::from([(target_identity.clone(), CallEdgeType::Strong)]);
        graph.rvs_insert_M(helper_identity.def_path.clone(), helper_node);
        graph.rvs_insert_M(target_identity.def_path.clone(), FnNode::default());
        let transitive_covered = rvs_direct_covered_functions(&graph);
        let covered = BTreeSet::new();
        let unique_names = BTreeMap::from([("rvs_run".to_string(), 1)]);
        let duplicate_names = BTreeMap::from([("rvs_run".to_string(), 2)]);
        let resolved =
            rvs_test_calls_function(&resolved_calls, &covered, &unique_names, &candidate);
        let wrong_crate =
            rvs_test_calls_function(&wrong_crate_calls, &covered, &unique_names, &candidate);
        let unresolved =
            rvs_test_calls_function(&unresolved_calls, &covered, &unique_names, &candidate);
        let ambiguous_unresolved = rvs_test_calls_function(
            &unresolved_calls,
            &covered,
            &duplicate_names,
            &duplicate_candidate,
        );
        let missing = rvs_test_calls_function(&missing_calls, &covered, &unique_names, &candidate);
        let output = format!(
            "resolved={resolved}\nwrong_crate={wrong_crate}\nunresolved={unresolved}\nambiguous_unresolved={ambiguous_unresolved}\nmissing={missing}\n"
        );
        rvs_snapshot_BIS("test_20260714_test_call_target_matching", &output);

        assert!(resolved);
        assert!(!wrong_crate);
        assert!(unresolved);
        assert!(!ambiguous_unresolved);
        assert!(!missing);
        assert!(transitive_covered.contains(&target_identity));
    }
}
