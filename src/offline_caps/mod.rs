use std::collections::{BTreeSet, HashMap};
use std::fmt;

use crate::artifacts::{FnGraph, FnNode};
use crate::capability::{Capability, CapabilitySet, ParsedFunctionName};
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

    pub(crate) fn rvs_is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl fmt::Display for OfflineCapsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.diagnostics.is_empty() {
            writeln!(f, "Offline Caps Check: ok")?;
            return Ok(());
        }

        let error_count = self
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == OfflineCapsSeverity::Error)
            .count();
        let warning_count = self.diagnostics.len().saturating_sub(error_count);
        writeln!(
            f,
            "Offline Caps Check: {error_count} error(s), {warning_count} warning(s)"
        )?;
        writeln!(f, "{:-<60}", "")?;
        for diagnostic in &self.diagnostics {
            writeln!(
                f,
                "{}[{}]: {}",
                diagnostic.severity.rvs_as_str(),
                diagnostic.kind.rvs_as_str(),
                diagnostic.function
            )?;
            writeln!(f, "  {}", diagnostic.message)?;
            for detail in &diagnostic.details {
                writeln!(f, "  {detail}")?;
            }
        }
        Ok(())
    }
}

struct OfflineFnContext<'a> {
    def_path: &'a DefPath,
    node: &'a FnNode,
    parsed_name: ParsedFunctionName<'a>,
    declared_caps: Option<CapabilitySet>,
    inferred_caps: Option<&'a CapabilitySet>,
    contract_diff: Option<&'a FnContractDiff>,
}

pub(crate) fn rvs_check_offline_caps_M(
    graph: &mut FnGraph,
    caps: &CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) -> OfflineCapsReport {
    let mut report = OfflineCapsReport::default();
    let local_scope = LocalScope::rvs_new(local_crate_names);
    let analysis = PreparedLocalAnalysis::rvs_prepare_M(graph, caps, local_crate_names);
    let diffs_by_path: HashMap<&str, &FnContractDiff> = analysis
        .diffs
        .iter()
        .map(|diff| (diff.def_path.rvs_as_str(), diff))
        .collect();
    let resolver = analysis.rvs_resolver(graph, caps);
    for (def_path, node) in graph.rvs_iter() {
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
        rvs_collect_call_diagnostics_M(&mut report, &context, &resolver);
    }
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
        let mut details = vec![format!("expected name: {}", diff.expected_name)];
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
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::Contract(kind),
            function: diff.def_path.clone(),
            message,
            details,
        });
    }
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
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::NonAlphabeticalSuffix,
            function: context.def_path.clone(),
            message: format!("suffix '{raw_suffix}' should be alphabetically ordered"),
            details: vec![format!("suggested suffix order: {sorted}")],
        });
    }
    if let Some(letter) = context.parsed_name.rvs_duplicate_suffix_letters().first() {
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::DuplicateSuffix,
            function: context.def_path.clone(),
            message: format!("suffix '{raw_suffix}' repeats '{letter}'"),
            details: vec!["remove duplicate capability letters".to_string()],
        });
    }
    let unknown = context.parsed_name.rvs_unknown_suffix_letters();
    if !unknown.is_empty() {
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::UnknownSuffixLetter,
            function: context.def_path.clone(),
            message: format!(
                "suffix '{raw_suffix}' contains unknown letters: {}",
                unknown
                    .iter()
                    .map(char::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            details: vec!["known letters are A, B, I, M, P, S, T, U".to_string()],
        });
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
    report.diagnostics.push(OfflineCapsDiagnostic {
        severity: OfflineCapsSeverity::Error,
        kind: OfflineCapsKind::StaticRefRequiresCaps,
        function: context.def_path.clone(),
        message: "function touches static/thread-local state without declaring required caps"
            .to_string(),
        details: vec![
            format!("declared caps: {}", rvs_format_caps(declared)),
            format!(
                "required caps from body facts: {}",
                rvs_format_caps(&required)
            ),
            format!("missing: {}", rvs_format_cap_list(&missing)),
        ],
    });
}

fn rvs_collect_call_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    resolver: &CalleeCapsResolver<'_>,
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
        let Some(mismatch) = rvs_collect_call_contract_mismatch(
            callee.rvs_as_str(),
            caller_caps,
            callee_caps.as_ref(),
        ) else {
            continue;
        };
        match mismatch.kind {
            CallContractMismatchKind::UnknownCallee => {
                report.diagnostics.push(OfflineCapsDiagnostic {
                    severity: OfflineCapsSeverity::Warning,
                    kind: OfflineCapsKind::UnknownCallee,
                    function: context.def_path.clone(),
                    message: format!(
                        "callee '{}' has no rvs_ suffix and no caps/ entry",
                        mismatch.callee_display
                    ),
                    details: vec![
                        format!("callee: {callee}"),
                        "add an exact def_path entry to caps/seed or caps/ext".to_string(),
                    ],
                });
            }
            CallContractMismatchKind::MissingCapabilities => {
                let callee_caps = mismatch
                    .callee_caps
                    .as_ref()
                    .expect("never: missing-capability mismatch carries callee caps");
                let missing: Vec<_> = mismatch.missing_caps.iter().copied().collect();
                report.diagnostics.push(OfflineCapsDiagnostic {
                    severity: OfflineCapsSeverity::Error,
                    kind: OfflineCapsKind::CallViolation,
                    function: context.def_path.clone(),
                    message: "caller lacks propagated capabilities required by callee".to_string(),
                    details: vec![
                        format!("callee: {callee}"),
                        format!("caller declared caps: {}", rvs_format_caps(caller_caps)),
                        format!("callee caps: {}", rvs_format_caps(callee_caps)),
                        format!("missing propagated caps: {}", rvs_format_cap_list(&missing)),
                    ],
                });
            }
        }
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
    let caps_str = crate::inference::rvs_caps_to_string(caps);
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

        let report = rvs_check_offline_caps_M(&mut graph, &caps, &local);
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260709_offline_caps_reports_call_violation_and_unknown",
            &output,
        );

        assert!(report.rvs_has_errors());
        assert!(output.contains("error[call_violation]"));
        assert!(output.contains("warning[unknown_callee]"));
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

        let report = rvs_check_offline_caps_M(&mut graph, &caps, &local);
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

        let report = rvs_check_offline_caps_M(&mut graph, &caps, &local);
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

        let report = rvs_check_offline_caps_M(&mut graph, &caps, &local);
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
}
