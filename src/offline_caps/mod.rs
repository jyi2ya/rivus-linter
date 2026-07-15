use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::artifacts::{FnGraph, FnNode, FunctionIdentity};
use crate::callgraph_cache::rvs_is_std_like_def_path;
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet, ParsedFunctionName};
use crate::capsmap::CapsMap;
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::inference::{
    CallContractMismatchKind, CalleeCapsResolver, FnContractDiff, FnContractMismatchKind,
    PreparedLocalAnalysis, rvs_collect_call_contract_mismatch,
};
use crate::symbols::{CrateName, DefPath};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OfflineCapsSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OfflineCapsKind {
    CallViolation,
    Contract(FnContractMismatchKind),
    DuplicateSuffix,
    NonAlphabeticalSuffix,
    StaticRefRequiresCaps,
    UnknownCallee,
    UnknownSuffixLetter,
}

impl OfflineCapsKind {
    pub(crate) fn rvs_as_str(self) -> &'static str {
        match self {
            Self::CallViolation => "call_violation",
            Self::Contract(kind) => kind.rvs_as_str(),
            Self::DuplicateSuffix => "duplicate_suffix",
            Self::NonAlphabeticalSuffix => "non_alphabetical_suffix",
            Self::StaticRefRequiresCaps => "static_ref_requires_caps",
            Self::UnknownCallee => "unknown_callee",
            Self::UnknownSuffixLetter => "unknown_suffix_letter",
        }
    }
}

impl OfflineCapsSeverity {
    fn rvs_as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OfflineCapsDiagnostic {
    pub(crate) severity: OfflineCapsSeverity,
    pub(crate) kind: OfflineCapsKind,
    pub(crate) function: DefPath,
    pub(crate) span_anchors: BTreeSet<DefPath>,
    pub(crate) message: String,
    pub(crate) details: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OfflineCapsReport {
    pub(crate) diagnostics: Vec<OfflineCapsDiagnostic>,
}

impl OfflineCapsReport {
    pub(crate) fn rvs_has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == OfflineCapsSeverity::Error)
    }

    #[cfg(test)]
    pub(crate) fn rvs_is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub(crate) fn rvs_render_with_title(&self, title: &str) -> String {
        let mut output = String::new();
        use std::fmt::Write as _;
        if self.diagnostics.is_empty() {
            writeln!(output, "{title}: ok").expect("never: writing to String cannot fail");
            return output;
        }

        let error_count = self
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == OfflineCapsSeverity::Error)
            .count();
        let warning_count = self.diagnostics.len().saturating_sub(error_count);
        writeln!(
            output,
            "{title}: {error_count} error(s), {warning_count} warning(s)"
        )
        .expect("never: writing to String cannot fail");
        writeln!(output, "{:-<60}", "").expect("never: writing to String cannot fail");
        for diagnostic in &self.diagnostics {
            writeln!(
                output,
                "{}[{}]: {}",
                diagnostic.severity.rvs_as_str(),
                diagnostic.kind.rvs_as_str(),
                diagnostic.function
            )
            .expect("never: writing to String cannot fail");
            writeln!(output, "  {}", diagnostic.message)
                .expect("never: writing to String cannot fail");
            for detail in &diagnostic.details {
                writeln!(output, "  {detail}").expect("never: writing to String cannot fail");
            }
        }
        output
    }
}

impl fmt::Display for OfflineCapsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rvs_render_with_title("Offline Caps Check"))
    }
}

#[derive(Debug)]
struct OfflineFnContext<'a> {
    def_path: &'a DefPath,
    node: &'a FnNode,
    parsed_name: ParsedFunctionName<'a>,
    declared_caps: Option<CapabilitySet>,
    inferred_caps: Option<&'a CapabilitySet>,
    contract_diff: Option<&'a FnContractDiff>,
}

impl OfflineFnContext<'_> {
    fn rvs_diagnostic(
        &self,
        severity: OfflineCapsSeverity,
        kind: OfflineCapsKind,
        message: String,
        details: Vec<String>,
    ) -> OfflineCapsDiagnostic {
        OfflineCapsDiagnostic {
            severity,
            kind,
            function: self.def_path.clone(),
            span_anchors: BTreeSet::from([self.def_path.clone()]),
            message,
            details,
        }
    }
}

pub(crate) fn rvs_check_offline_caps(
    graph: &FnGraph,
    caps: &CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) -> OfflineCapsReport {
    let mut report = OfflineCapsReport::default();
    let local_scope = LocalScope::rvs_new(local_crate_names);
    let mut scoped_graph = graph.clone();
    let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, caps, local_crate_names);
    let diffs_by_path: HashMap<&str, &FnContractDiff> = analysis
        .diffs
        .iter()
        .map(|diff| (diff.def_path.rvs_as_str(), diff))
        .collect();
    let resolver = analysis.rvs_resolver(&scoped_graph, caps);
    let mut unknown_callees: BTreeMap<String, BTreeSet<DefPath>> = BTreeMap::new();
    for (def_path, node) in scoped_graph.rvs_iter() {
        let classification = FunctionClassification::rvs_new(&local_scope, def_path, node);
        if !classification.rvs_is_offline_checked() {
            continue;
        }
        let parsed_name = ParsedFunctionName::rvs_parse(def_path.rvs_as_str());
        let declared_caps = parsed_name.rvs_declared_caps();
        let context = OfflineFnContext {
            def_path,
            node,
            parsed_name,
            declared_caps,
            inferred_caps: analysis.rvs_inferred().get(def_path),
            contract_diff: diffs_by_path.get(def_path.rvs_as_str()).copied(),
        };
        rvs_collect_contract_diagnostics_M(&mut report, &context);
        rvs_collect_suffix_diagnostics_M(&mut report, &context);
        rvs_collect_static_ref_diagnostics_M(&mut report, &context);
        rvs_collect_call_diagnostics_M(&mut report, &context, &resolver, &mut unknown_callees);
    }
    rvs_append_unknown_callee_diagnostics_M(&mut report, &unknown_callees);
    report.diagnostics.sort();
    report
}

fn rvs_collect_contract_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
) {
    let Some(diff) = context.contract_diff else {
        return;
    };
    let mismatch_kinds = diff.rvs_mismatch_kinds();
    let selected: Vec<_> = if mismatch_kinds.contains(&FnContractMismatchKind::MissingRvsPrefix) {
        vec![FnContractMismatchKind::MissingRvsPrefix]
    } else if mismatch_kinds.contains(&FnContractMismatchKind::NameMismatch)
        && diff.expected_public_caps.rvs_contains(Capability::P)
    {
        vec![FnContractMismatchKind::NameMismatch]
    } else {
        mismatch_kinds
            .into_iter()
            .filter(|kind| *kind != FnContractMismatchKind::NameMismatch)
            .collect()
    };
    for kind in selected {
        let mut details = Vec::new();
        if kind != FnContractMismatchKind::NameMismatch {
            details.push(format!("expected name: {}", diff.expected_name));
        }
        details.push(format!(
            "declared caps: {}",
            rvs_format_optional_caps(diff.declared_public_caps.as_ref())
        ));
        details.push(format!(
            "inferred caps: {}",
            rvs_format_caps(&diff.expected_public_caps)
        ));
        let message = match kind {
            FnContractMismatchKind::MissingRvsPrefix => {
                format!("'{}' is missing the rvs_ prefix", diff.actual_name)
            }
            FnContractMismatchKind::NameMismatch => format!(
                "'{}' should be named '{}'",
                diff.actual_name, diff.expected_name
            ),
            kind => format!(
                "'{}' is missing capability marker {}",
                diff.actual_name,
                kind.rvs_as_str()
            ),
        };
        report.diagnostics.push(context.rvs_diagnostic(
            OfflineCapsSeverity::Warning,
            OfflineCapsKind::Contract(kind),
            message,
            details,
        ));
    }
}

pub(crate) fn rvs_uncovered_test_functions(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> BTreeSet<FunctionIdentity> {
    let local_scope = LocalScope::rvs_new(local_crate_names);
    let unresolved_test_calls: BTreeSet<&str> = graph
        .rvs_iter()
        .filter(|(_, node)| !node.test_crate_ids.is_empty())
        .flat_map(|(_, node)| node.unresolved_test_calls.iter().map(String::as_str))
        .collect();
    let covered: BTreeSet<FunctionIdentity> = graph
        .rvs_test_reachable_identities()
        .into_iter()
        .map(|identity| rvs_normalize_coverage_identity(graph, &identity))
        .collect();

    let mut candidates = Vec::new();
    for (def_path, node) in graph.rvs_iter() {
        if !local_scope.rvs_contains(def_path)
            || !node.has_body
            || node.coverage_candidate_crate_ids.is_empty()
        {
            continue;
        }
        let parsed = ParsedFunctionName::rvs_parse(def_path.rvs_as_str());
        if !parsed.rvs_has_rvs_prefix() {
            continue;
        }
        let caps = if node.facts.is_port_method {
            CapabilityPolicy::rvs_port_method_caps()
        } else {
            parsed.rvs_known_caps().clone()
        };
        if CapabilityPolicy::rvs_is_ok(&caps) {
            candidates.extend(node.coverage_candidate_crate_ids.iter().map(|crate_id| {
                FunctionIdentity {
                    crate_id: *crate_id,
                    def_path: def_path.clone(),
                }
            }));
        }
    }
    let mut candidate_name_counts = HashMap::new();
    for identity in &candidates {
        *candidate_name_counts
            .entry(identity.def_path.rvs_fn_name_str().to_string())
            .or_insert(0usize) += 1;
    }
    let mut uncovered = BTreeSet::new();

    for identity in candidates {
        let name = identity.def_path.rvs_fn_name_str();
        let uniquely_covered_by_name = unresolved_test_calls.contains(name)
            && candidate_name_counts.get(name).copied() == Some(1);
        if !covered.contains(&identity) && !uniquely_covered_by_name {
            if let Some(node) = graph.rvs_get(identity.def_path.rvs_as_str()) {
                uncovered.extend(rvs_test_compilation_aliases(node, &identity));
            }
            uncovered.insert(identity);
        }
    }
    uncovered
}

fn rvs_test_compilation_aliases(
    node: &FnNode,
    production: &FunctionIdentity,
) -> Vec<FunctionIdentity> {
    let production_sources = node.sources_by_crate.get(&production.crate_id);
    node.sources_by_crate
        .iter()
        .filter(|(crate_id, sources)| {
            !node.production_crate_ids.contains(crate_id)
                && production_sources.is_some_and(|production_sources| {
                    (!production_sources.is_empty() && !production_sources.is_disjoint(sources))
                        || (production_sources.is_empty()
                            && sources.is_empty()
                            && node.production_crate_ids.len() == 1)
                })
        })
        .map(|(crate_id, _)| FunctionIdentity {
            crate_id: *crate_id,
            def_path: production.def_path.clone(),
        })
        .collect()
}

fn rvs_normalize_coverage_identity(
    graph: &FnGraph,
    identity: &FunctionIdentity,
) -> FunctionIdentity {
    let Some(node) = graph.rvs_get(identity.def_path.rvs_as_str()) else {
        return identity.clone();
    };
    if node.production_crate_ids.contains(&identity.crate_id) {
        return identity.clone();
    }
    let source_matches: Vec<u64> = node
        .sources_by_crate
        .get(&identity.crate_id)
        .into_iter()
        .flat_map(|sources| {
            node.production_crate_ids
                .iter()
                .filter(|crate_id| {
                    node.sources_by_crate
                        .get(crate_id)
                        .is_some_and(|production_sources| !sources.is_disjoint(production_sources))
                })
                .copied()
        })
        .collect();
    let production_crate_id = match source_matches.as_slice() {
        [crate_id] => Some(*crate_id),
        [] if node
            .sources_by_crate
            .get(&identity.crate_id)
            .is_none_or(BTreeSet::is_empty)
            && node.production_crate_ids.len() == 1 =>
        {
            node.production_crate_ids.first().copied()
        }
        _ => None,
    };
    production_crate_id.map_or_else(
        || identity.clone(),
        |crate_id| FunctionIdentity {
            crate_id,
            def_path: identity.def_path.clone(),
        },
    )
}

fn rvs_collect_suffix_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
) {
    let Some(raw_suffix) = context.parsed_name.rvs_raw_suffix() else {
        return;
    };
    if !context.parsed_name.rvs_suffix_is_canonical() {
        let sorted = context
            .parsed_name
            .rvs_canonical_suffix()
            .expect("never: raw suffix has a canonical form");
        report.diagnostics.push(context.rvs_diagnostic(
            OfflineCapsSeverity::Warning,
            OfflineCapsKind::NonAlphabeticalSuffix,
            format!("suffix '{raw_suffix}' should be alphabetically ordered"),
            vec![format!("suggested suffix order: {sorted}")],
        ));
    }
    if let Some(letter) = context.parsed_name.rvs_duplicate_suffix_letters().first() {
        report.diagnostics.push(context.rvs_diagnostic(
            OfflineCapsSeverity::Warning,
            OfflineCapsKind::DuplicateSuffix,
            format!("suffix '{raw_suffix}' repeats '{letter}'"),
            vec!["remove duplicate capability letters".to_string()],
        ));
    }
    let unknown = context.parsed_name.rvs_unknown_suffix_letters();
    if !unknown.is_empty() {
        report.diagnostics.push(context.rvs_diagnostic(
            OfflineCapsSeverity::Warning,
            OfflineCapsKind::UnknownSuffixLetter,
            format!(
                "suffix '{raw_suffix}' contains unknown letters: {}",
                unknown
                    .iter()
                    .map(char::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            vec!["known letters are A, B, I, M, P, S, T, U".to_string()],
        ));
    }
}

fn rvs_collect_static_ref_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
) {
    let Some(declared) = context.declared_caps.as_ref() else {
        return;
    };
    let mut required = CapabilitySet::rvs_new();
    if context.node.facts.has_static_ref || context.node.facts.has_static_mut_ref {
        required.rvs_insert_M(Capability::S);
    }
    if context.node.facts.has_static_mut_ref {
        required.rvs_insert_M(Capability::U);
    }
    if context.node.facts.has_thread_local_ref {
        required.rvs_insert_M(Capability::T);
    }
    let missing: Vec<_> = [Capability::S, Capability::T, Capability::U]
        .into_iter()
        .filter(|cap| required.rvs_contains(*cap) && !declared.rvs_contains(*cap))
        .collect();
    if missing.is_empty() {
        return;
    }
    report.diagnostics.push(context.rvs_diagnostic(
        OfflineCapsSeverity::Error,
        OfflineCapsKind::StaticRefRequiresCaps,
        "function touches static/thread-local state without declaring required caps".to_string(),
        vec![
            format!("declared caps: {}", rvs_format_caps(declared)),
            format!(
                "required caps from body facts: {}",
                rvs_format_caps(&required)
            ),
            format!("missing: {}", rvs_format_cap_list(&missing)),
        ],
    ));
}

fn rvs_collect_call_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    resolver: &CalleeCapsResolver<'_>,
    unknown_callees: &mut BTreeMap<String, BTreeSet<DefPath>>,
) {
    if !context.node.has_body {
        return;
    }
    let Some(caller_caps) = context.declared_caps.as_ref().or(context.inferred_caps) else {
        return;
    };
    for callee in &context.node.calls {
        if rvs_is_test_harness_callee(callee) {
            continue;
        }
        let callee_caps = resolver.rvs_for_contract_check(callee);
        let callee_display = callee.to_string();
        let Some(mismatch) =
            rvs_collect_call_contract_mismatch(&callee_display, caller_caps, callee_caps.as_ref())
        else {
            continue;
        };
        match mismatch.kind {
            CallContractMismatchKind::UnknownCallee => {
                unknown_callees
                    .entry(mismatch.callee_display)
                    .or_default()
                    .insert(context.def_path.clone());
            }
            CallContractMismatchKind::MissingCapabilities => {
                let callee_caps = mismatch
                    .callee_caps
                    .as_ref()
                    .expect("never: missing-capability mismatch carries callee caps");
                let missing: Vec<_> = mismatch.missing_caps.iter().copied().collect();
                report.diagnostics.push(context.rvs_diagnostic(
                    OfflineCapsSeverity::Error,
                    OfflineCapsKind::CallViolation,
                    "caller lacks propagated capabilities required by callee".to_string(),
                    vec![
                        format!("callee: {callee}"),
                        format!("caller declared caps: {}", rvs_format_caps(caller_caps)),
                        format!("callee caps: {}", rvs_format_caps(callee_caps)),
                        format!("missing propagated caps: {}", rvs_format_cap_list(&missing)),
                    ],
                ));
            }
        }
    }
}

fn rvs_append_unknown_callee_diagnostics_M(
    report: &mut OfflineCapsReport,
    unknown_callees: &BTreeMap<String, BTreeSet<DefPath>>,
) {
    for (callee, callers) in unknown_callees {
        let readable_callers: BTreeSet<String> = callers.iter().map(ToString::to_string).collect();
        let mut details: Vec<String> = readable_callers
            .iter()
            .take(5)
            .map(|caller| format!("called by: {caller}"))
            .collect();
        if readable_callers.len() > 5 {
            details.push(format!(
                "... and {} more callers",
                readable_callers.len() - 5
            ));
        }
        let callee_path = DefPath::from(callee.as_str());
        details.push(rvs_unknown_callee_repair(&callee_path));
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::UnknownCallee,
            function: callee_path,
            span_anchors: callers.clone(),
            message: format!("callee '{callee}' has no rvs_ suffix and no caps/ entry"),
            details,
        });
    }
}

fn rvs_unknown_callee_repair(callee: &DefPath) -> String {
    if rvs_is_std_like_def_path(callee.rvs_as_str()) {
        "run `cargo rivus infer-std -o caps/std` to refresh standard-library capabilities; if that command reports an unknown prerequisite, add its exact def_path to caps/seed; use caps/ext only for a project-local check override".to_string()
    } else {
        "run `cargo rivus infer-capsmap -o caps/deps` to refresh dependency capabilities; if inference still reports this path, add its exact def_path to caps/ext".to_string()
    }
}

fn rvs_is_test_harness_callee(callee: &DefPath) -> bool {
    callee.rvs_as_str() == "test::test_main_static"
}

fn rvs_format_optional_caps(caps: Option<&CapabilitySet>) -> String {
    caps.map(rvs_format_caps)
        .unwrap_or_else(|| "unknown".to_string())
}

fn rvs_format_caps(caps: &CapabilitySet) -> String {
    let caps_str = caps.rvs_letters();
    if caps_str.is_empty() {
        "pure".to_string()
    } else {
        caps_str
    }
}

fn rvs_format_cap_list(caps: &[Capability]) -> String {
    if caps.is_empty() {
        return "none".to_string();
    }
    caps.iter()
        .map(|cap| cap.rvs_as_char().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{FnNode, FnSource};
    use crate::capability::CapabilityFacts;
    use crate::test_support::rvs_snapshot_BIS;
    use std::path::PathBuf;

    fn rvs_node(calls: &[&str]) -> FnNode {
        let mut node = FnNode::default();
        node.calls = calls.iter().map(|call| DefPath::from(*call)).collect();
        node.sources
            .insert(FnSource::rvs_new(PathBuf::from("src/lib.rs"), 1, 2));
        node.coverage_calls.insert(
            1,
            calls
                .iter()
                .map(|call| FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from(*call),
                })
                .collect(),
        );
        node.production_crate_ids.insert(1);
        node.coverage_candidate_crate_ids.insert(1);
        node.sources_by_crate.insert(1, node.sources.clone());
        node
    }

    #[test]
    fn test_20260709_offline_caps_reports_call_violation_and_unknown() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["std::fs::read_to_string", "dep::plain"]),
        );
        let caps = CapsMap::rvs_parse("std::fs::read_to_string=BI\n").unwrap();
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260709_offline_caps_reports_call_violation_and_unknown",
            &output,
        );

        assert!(report.rvs_has_errors());
        assert!(!report.rvs_is_empty());
        assert!(
            report
                .rvs_render_with_title("Custom Caps Check")
                .starts_with("Custom Caps Check:")
        );
        assert_eq!(
            OfflineCapsKind::CallViolation.rvs_as_str(),
            "call_violation"
        );
        assert_eq!(OfflineCapsSeverity::Warning.rvs_as_str(), "warning");
        assert!(output.contains("error[call_violation]"));
        assert!(output.contains("warning[unknown_callee]"));
        assert!(output.contains("cargo rivus infer-capsmap -o caps/deps"));
    }

    #[test]
    fn test_20260715_empty_offline_caps_report_renders_stage_success() {
        let output = OfflineCapsReport::default().to_string();
        rvs_snapshot_BIS(
            "test_20260715_empty_offline_caps_report_renders_stage_success",
            &output,
        );

        assert_eq!(output, "Offline Caps Check: ok\n");
    }

    #[test]
    fn test_20260714_offline_caps_unknown_std_callee_suggests_std_caps() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["core::slice::iter"]),
        );
        let caps = CapsMap::rvs_new();
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260714_offline_caps_unknown_std_callee_suggests_std_caps",
            &output,
        );

        assert!(output.contains("cargo rivus infer-std -o caps/std"));
        assert!(output.contains("add its exact def_path to caps/seed"));
        assert!(output.contains("use caps/ext only for a project-local check override"));
    }

    #[test]
    fn test_20260715_offline_unknown_callee_hides_impl_marker() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["dep::Worker{impl#6465703a3a576f726b65723c75383e}::run"]),
        );
        let local = BTreeSet::from([CrateName::from("demo")]);

        let output = rvs_check_offline_caps(&graph, &CapsMap::rvs_new(), &local).to_string();
        rvs_snapshot_BIS(
            "test_20260715_offline_unknown_callee_hides_impl_marker",
            &output,
        );

        assert!(output.contains("callee 'dep::Worker::run'"));
        assert!(!output.contains("{impl#"));
    }

    #[test]
    fn test_20260715_offline_unknown_callees_group_callers_by_callee() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_first"), rvs_node(&["dep::plain"]));
        graph.rvs_insert_M(DefPath::from("demo::rvs_second"), rvs_node(&["dep::plain"]));
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &CapsMap::rvs_new(), &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260715_offline_unknown_callees_group_callers_by_callee",
            &output,
        );

        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee)
                .count(),
            1
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee)
            .expect("never: grouped report contains the unknown-callee diagnostic");
        assert_eq!(diagnostic.span_anchors.len(), 2);
    }

    #[test]
    fn test_20260715_offline_unknown_callee_deduplicates_readable_callers() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Worker{impl#7538}::rvs_call"),
            rvs_node(&["dep::plain"]),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Worker{impl#753136}::rvs_call"),
            rvs_node(&["dep::plain"]),
        );
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &CapsMap::rvs_new(), &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260715_offline_unknown_callee_deduplicates_readable_callers",
            &output,
        );

        assert_eq!(
            output.matches("called by: demo::Worker::rvs_call").count(),
            1
        );
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee)
            .expect("never: grouped report contains the unknown-callee diagnostic");
        assert_eq!(diagnostic.span_anchors.len(), 2);
    }

    #[test]
    fn test_20260714_offline_caps_preserves_port_facts() {
        let mut graph = FnGraph::rvs_new();
        let mut app = rvs_node(&[]);
        app.facts.is_port_method = true;
        let mut dependency = rvs_node(&[]);
        dependency.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("app::ApiClient::rvs_fetch_P"), app);
        graph.rvs_insert_M(
            DefPath::from("dependency::HttpClient::rvs_fetch_P"),
            dependency,
        );

        let _ = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("app")]),
        );
        let app_port = graph
            .rvs_get("app::ApiClient::rvs_fetch_P")
            .is_some_and(|node| node.facts.is_port_method);
        let dependency_port = graph
            .rvs_get("dependency::HttpClient::rvs_fetch_P")
            .is_some_and(|node| node.facts.is_port_method);
        let output = format!("app={app_port}\ndependency={dependency_port}\n");
        rvs_snapshot_BIS("test_20260714_offline_caps_preserves_port_facts", &output);

        assert!(app_port);
        assert!(dependency_port);
    }

    #[test]
    fn test_20260709_offline_caps_reports_contract_and_static_ref() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_node(&[]);
        node.facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        graph.rvs_insert_M(DefPath::from("demo::read_cache"), node);
        let caps = CapsMap::rvs_new();
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260709_offline_caps_reports_contract_and_static_ref",
            &output,
        );

        assert!(output.contains("warning[missing_rvs_prefix]"));
        assert!(output.contains("expected name: rvs_read_cache_S"));
    }

    #[test]
    fn test_20260713_port_trait_impl_offline_checked_without_contract_diff() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_node(&["dep::effect"]);
        node.is_trait_impl = true;
        node.facts.is_port_method = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Adapter::rvs_fetch_P@demo::ApiClient"),
            node,
        );
        let caps = CapsMap::rvs_parse("dep::effect=S\n").unwrap();
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut analysis_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut analysis_graph, &caps, &local);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = format!("contract_diffs={}\n{}", analysis.diffs.len(), report);
        rvs_snapshot_BIS(
            "test_20260713_port_trait_impl_offline_checked_without_contract_diff",
            &output,
        );

        assert!(analysis.diffs.is_empty());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == OfflineCapsKind::CallViolation)
        );
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.kind, OfflineCapsKind::Contract(_)))
        );
    }

    #[test]
    fn test_20260713_offline_caps_single_context_emits_all_diagnostic_families() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_node(&["dep::effect"]);
        node.facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        graph.rvs_insert_M(DefPath::from("demo::rvs_handle_IBBZ"), node);
        let caps = CapsMap::rvs_parse("dep::effect=S\n").unwrap();
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260713_offline_caps_single_context_emits_all_diagnostic_families",
            &output,
        );

        for kind in [
            OfflineCapsKind::CallViolation,
            OfflineCapsKind::Contract(FnContractMismatchKind::MissingSideEffect),
            OfflineCapsKind::DuplicateSuffix,
            OfflineCapsKind::NonAlphabeticalSuffix,
            OfflineCapsKind::StaticRefRequiresCaps,
            OfflineCapsKind::UnknownSuffixLetter,
        ] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.kind == kind),
                "missing diagnostic family: {kind:?}"
            );
        }
    }

    #[test]
    fn test_20260714_test_coverage_uses_merged_targets() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_covered"), rvs_node(&[]));
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_transitive"),
            rvs_node(&["demo::rvs_transitive_helper"]),
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_transitive_helper"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::rvs_name_fallback"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::one::rvs_ambiguous"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::two::rvs_ambiguous"), rvs_node(&[]));
        let mut cfg_wrapper = rvs_node(&["demo::rvs_cfg_production_only"]);
        cfg_wrapper.coverage_calls.insert(
            2,
            BTreeSet::from([FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("demo::rvs_cfg_test_only"),
            }]),
        );
        cfg_wrapper
            .sources_by_crate
            .insert(2, cfg_wrapper.sources.clone());
        graph.rvs_insert_M(DefPath::from("demo::rvs_cfg_wrapper"), cfg_wrapper);
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_cfg_production_only"),
            rvs_node(&[]),
        );
        let mut cfg_test_only = rvs_node(&[]);
        cfg_test_only
            .sources_by_crate
            .insert(2, cfg_test_only.sources.clone());
        graph.rvs_insert_M(DefPath::from("demo::rvs_cfg_test_only"), cfg_test_only);
        let mut generated = rvs_node(&[]);
        generated.sources.clear();
        generated.sources_by_crate.insert(1, BTreeSet::new());
        generated.sources_by_crate.insert(2, BTreeSet::new());
        graph.rvs_insert_M(DefPath::from("demo::rvs_generated"), generated);
        graph.rvs_insert_M(DefPath::from("demo::rvs_uncovered"), rvs_node(&[]));
        let mut partially_allowed = rvs_node(&[]);
        partially_allowed.allows_dead_code = true;
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_partially_allowed"),
            partially_allowed,
        );
        let mut test_only_helper = rvs_node(&[]);
        test_only_helper.is_test_compilation = true;
        test_only_helper.production_crate_ids.clear();
        test_only_helper.coverage_candidate_crate_ids.clear();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_test_only_helper"),
            test_only_helper,
        );

        let mut test_node = rvs_node(&["demo::rvs_covered", "demo::rvs_transitive"]);
        test_node.is_test = true;
        test_node.is_test_compilation = true;
        test_node.test_crate_ids.insert(1);
        test_node.production_crate_ids.clear();
        test_node.coverage_candidate_crate_ids.clear();
        test_node
            .unresolved_test_calls
            .insert("rvs_name_fallback".to_string());
        test_node
            .unresolved_test_calls
            .insert("rvs_ambiguous".to_string());
        test_node.coverage_calls.get_mut(&1).unwrap().extend([
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("demo::rvs_generated"),
            },
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("demo::rvs_cfg_wrapper"),
            },
        ]);
        graph.rvs_insert_M(
            DefPath::from("integration_test::test_20260714_calls_library"),
            test_node,
        );
        let local = BTreeSet::from([CrateName::from("demo"), CrateName::from("integration_test")]);

        let uncovered = rvs_uncovered_test_functions(&graph, &local);
        let output = uncovered
            .iter()
            .map(|identity| identity.def_path.rvs_as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260714_test_coverage_uses_merged_targets", &output);

        assert_eq!(
            uncovered,
            BTreeSet::from([
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::one::rvs_ambiguous"),
                },
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_cfg_production_only"),
                },
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_uncovered"),
                },
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_partially_allowed"),
                },
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::two::rvs_ambiguous"),
                },
            ])
        );
    }
}
