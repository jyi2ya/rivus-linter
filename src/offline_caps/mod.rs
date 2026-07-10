use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::artifacts::{FnGraph, FnNode};
use crate::capability::{self, Capability, CapabilityPolicy, CapabilitySet};
use crate::capsmap::CapsMap;
use crate::inference::{
    CallContractMismatchKind, FnContractMismatchKind, rvs_collect_call_contract_mismatch,
    rvs_collect_enforced_contract_diffs, rvs_collect_local_contract_diffs_M,
    rvs_resolve_impl_majority_caps,
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
    DuplicateSuffix,
    MissingAsync,
    MissingBlocking,
    MissingIo,
    MissingMutable,
    MissingPort,
    MissingRvsPrefix,
    MissingSideEffect,
    MissingThreadLocal,
    MissingUnsafe,
    NameMismatch,
    NonAlphabeticalSuffix,
    StaticRefRequiresCaps,
    UnknownCallee,
    UnknownSuffixLetter,
}

impl OfflineCapsKind {
    pub(crate) fn rvs_as_str(self) -> &'static str {
        match self {
            Self::CallViolation => "call_violation",
            Self::DuplicateSuffix => "duplicate_suffix",
            Self::MissingAsync => "missing_async",
            Self::MissingBlocking => "missing_blocking",
            Self::MissingIo => "missing_io",
            Self::MissingMutable => "missing_mutable",
            Self::MissingPort => "missing_port",
            Self::MissingRvsPrefix => "missing_rvs_prefix",
            Self::MissingSideEffect => "missing_side_effect",
            Self::MissingThreadLocal => "missing_thread_local",
            Self::MissingUnsafe => "missing_unsafe",
            Self::NameMismatch => "name_mismatch",
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

pub(crate) fn rvs_check_offline_caps_M(
    graph: &mut FnGraph,
    caps: &CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) -> OfflineCapsReport {
    let mut report = OfflineCapsReport::default();
    let diffs = rvs_collect_local_contract_diffs_M(graph, caps, local_crate_names);
    rvs_collect_contract_diagnostics_M(&mut report, graph, &diffs, local_crate_names);
    rvs_collect_suffix_diagnostics_M(&mut report, graph, local_crate_names);
    rvs_collect_static_ref_diagnostics_M(&mut report, graph, local_crate_names);
    rvs_collect_call_diagnostics_M(&mut report, graph, caps, local_crate_names);
    report.diagnostics.sort();
    report
}

fn rvs_contract_kind(kind: FnContractMismatchKind) -> OfflineCapsKind {
    match kind {
        FnContractMismatchKind::MissingRvsPrefix => OfflineCapsKind::MissingRvsPrefix,
        FnContractMismatchKind::NameMismatch => OfflineCapsKind::NameMismatch,
        FnContractMismatchKind::MissingAsync => OfflineCapsKind::MissingAsync,
        FnContractMismatchKind::MissingBlocking => OfflineCapsKind::MissingBlocking,
        FnContractMismatchKind::MissingIo => OfflineCapsKind::MissingIo,
        FnContractMismatchKind::MissingMutable => OfflineCapsKind::MissingMutable,
        FnContractMismatchKind::MissingPort => OfflineCapsKind::MissingPort,
        FnContractMismatchKind::MissingSideEffect => OfflineCapsKind::MissingSideEffect,
        FnContractMismatchKind::MissingThreadLocal => OfflineCapsKind::MissingThreadLocal,
        FnContractMismatchKind::MissingUnsafe => OfflineCapsKind::MissingUnsafe,
    }
}

fn rvs_collect_contract_diagnostics_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    diffs: &[crate::inference::FnContractDiff],
    local_crate_names: &BTreeSet<CrateName>,
) {
    let enforced = rvs_collect_enforced_contract_diffs(graph, diffs, local_crate_names);
    for diff in &enforced {
        let Some(node) = graph.rvs_get(diff.def_path.rvs_as_str()) else {
            continue;
        };
        if !rvs_is_local_checked_fn(&diff.def_path, node, local_crate_names) {
            continue;
        }
        let mismatch_kinds = diff.rvs_mismatch_kinds();
        let selected: Vec<_> = if mismatch_kinds.contains(&FnContractMismatchKind::MissingRvsPrefix)
        {
            vec![FnContractMismatchKind::MissingRvsPrefix]
        } else if mismatch_kinds.contains(&FnContractMismatchKind::NameMismatch)
            && diff
                .expected_public_caps
                .as_ref()
                .is_some_and(|caps| caps.rvs_contains(Capability::P))
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
            if let Some(expected) = diff.expected_name.as_ref() {
                details.push(format!("expected name: {expected}"));
            }
            details.push(format!(
                "declared caps: {}",
                rvs_format_optional_caps(diff.declared_public_caps.as_ref())
            ));
            details.push(format!(
                "inferred caps: {}",
                rvs_format_optional_caps(diff.expected_public_caps.as_ref())
            ));
            let message = match kind {
                FnContractMismatchKind::MissingRvsPrefix => {
                    format!("'{}' is missing the rvs_ prefix", diff.actual_name)
                }
                FnContractMismatchKind::NameMismatch => format!(
                    "'{}' should be named '{}'",
                    diff.actual_name,
                    diff.expected_name
                        .as_ref()
                        .expect("never: name mismatch carries expected name")
                ),
                kind => format!(
                    "'{}' is missing capability marker {}",
                    diff.actual_name,
                    kind.rvs_as_str()
                ),
            };
            report.diagnostics.push(OfflineCapsDiagnostic {
                severity: OfflineCapsSeverity::Warning,
                kind: rvs_contract_kind(kind),
                function: diff.def_path.clone(),
                message,
                details,
            });
        }
    }
}

fn rvs_collect_suffix_diagnostics_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    for (def_path, node) in graph.rvs_iter() {
        if !rvs_is_local_checked_fn(def_path, node, local_crate_names) {
            continue;
        }
        let fn_name = def_path.rvs_fn_name();
        let raw_suffix = capability::rvs_extract_raw_suffix(fn_name.rvs_as_str());
        if raw_suffix.is_empty() {
            continue;
        }
        let sorted = {
            let mut chars: Vec<char> = raw_suffix.chars().collect();
            chars.sort_unstable();
            chars.into_iter().collect::<String>()
        };
        if raw_suffix != sorted {
            report.diagnostics.push(OfflineCapsDiagnostic {
                severity: OfflineCapsSeverity::Warning,
                kind: OfflineCapsKind::NonAlphabeticalSuffix,
                function: def_path.clone(),
                message: format!("suffix '{raw_suffix}' should be alphabetically ordered"),
                details: vec![format!("suggested suffix order: {sorted}")],
            });
        }
        let mut seen = BTreeSet::new();
        for letter in raw_suffix.chars() {
            if !seen.insert(letter) {
                report.diagnostics.push(OfflineCapsDiagnostic {
                    severity: OfflineCapsSeverity::Warning,
                    kind: OfflineCapsKind::DuplicateSuffix,
                    function: def_path.clone(),
                    message: format!("suffix '{raw_suffix}' repeats '{letter}'"),
                    details: vec!["remove duplicate capability letters".to_string()],
                });
                break;
            }
        }
        let unknown = capability::rvs_extract_unknown_suffix_letters(&raw_suffix);
        if !unknown.is_empty() {
            report.diagnostics.push(OfflineCapsDiagnostic {
                severity: OfflineCapsSeverity::Warning,
                kind: OfflineCapsKind::UnknownSuffixLetter,
                function: def_path.clone(),
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
}

fn rvs_collect_static_ref_diagnostics_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    for (def_path, node) in graph.rvs_iter() {
        if !rvs_is_local_checked_fn(def_path, node, local_crate_names) {
            continue;
        }
        let Some(declared) = rvs_declared_caps(def_path) else {
            continue;
        };
        let mut required = CapabilitySet::rvs_new();
        if node.facts.has_static_ref || node.facts.has_static_mut_ref {
            required.rvs_insert_M(Capability::S);
        }
        if node.facts.has_static_mut_ref {
            required.rvs_insert_M(Capability::U);
        }
        if node.facts.has_thread_local_ref {
            required.rvs_insert_M(Capability::T);
        }
        let missing: Vec<_> = [Capability::S, Capability::T, Capability::U]
            .into_iter()
            .filter(|cap| required.rvs_contains(*cap) && !declared.rvs_contains(*cap))
            .collect();
        if missing.is_empty() {
            continue;
        }
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Error,
            kind: OfflineCapsKind::StaticRefRequiresCaps,
            function: def_path.clone(),
            message: "function touches static/thread-local state without declaring required caps"
                .to_string(),
            details: vec![
                format!("declared caps: {}", rvs_format_caps(&declared)),
                format!(
                    "required caps from body facts: {}",
                    rvs_format_caps(&required)
                ),
                format!("missing: {}", rvs_format_cap_list(&missing)),
            ],
        });
    }
}

fn rvs_collect_call_diagnostics_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    caps: &CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let impl_index = crate::inference::rvs_build_impl_index(graph);
    let inferred = graph.rvs_expected_public_caps_map();
    for (caller, node) in graph.rvs_iter() {
        if !rvs_is_local_checked_fn(caller, node, local_crate_names) || !node.has_body {
            continue;
        }
        let Some(caller_caps) = rvs_declared_caps(caller).or_else(|| inferred.get(caller).cloned())
        else {
            continue;
        };
        for callee in &node.calls {
            if rvs_is_test_harness_callee(callee) {
                continue;
            }
            let callee_caps = rvs_lookup_callee_caps(callee, graph, caps, &inferred, &impl_index);
            let Some(mismatch) = rvs_collect_call_contract_mismatch(
                callee.rvs_as_str(),
                None,
                &caller_caps,
                callee_caps.as_ref(),
            ) else {
                continue;
            };
            match mismatch.kind {
                CallContractMismatchKind::UnknownCallee => {
                    report.diagnostics.push(OfflineCapsDiagnostic {
                        severity: OfflineCapsSeverity::Warning,
                        kind: OfflineCapsKind::UnknownCallee,
                        function: caller.clone(),
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
                        function: caller.clone(),
                        message: "caller lacks propagated capabilities required by callee"
                            .to_string(),
                        details: vec![
                            format!("callee: {callee}"),
                            format!("caller declared caps: {}", rvs_format_caps(&caller_caps)),
                            format!("callee caps: {}", rvs_format_caps(callee_caps)),
                            format!("missing propagated caps: {}", rvs_format_cap_list(&missing)),
                        ],
                    });
                }
            }
        }
    }
}

fn rvs_lookup_callee_caps(
    callee: &DefPath,
    graph: &FnGraph,
    caps: &CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &std::collections::HashMap<String, Vec<DefPath>>,
) -> Option<CapabilitySet> {
    if let Some(node) = graph.rvs_get(callee.rvs_as_str())
        && node.facts.is_port_method
    {
        return Some(CapabilityPolicy::rvs_port_method_caps());
    }
    caps.rvs_lookup(callee.rvs_as_str())
        .cloned()
        .or_else(|| rvs_declared_caps(callee))
        .or_else(|| inferred.get(callee).cloned())
        .or_else(|| {
            if callee.rvs_as_str().contains('@') {
                None
            } else {
                rvs_resolve_impl_majority_caps(callee, impl_index, inferred, graph)
            }
        })
}

fn rvs_is_local_checked_fn(
    def_path: &DefPath,
    node: &FnNode,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    if local_crate_names
        .iter()
        .any(|name| def_path == &name.rvs_prefix().rvs_join_name(&"main".into()))
    {
        return false;
    }
    if node.is_synthetic {
        return false;
    }
    if node.is_test {
        return false;
    }
    if node.is_trait_impl && !node.facts.is_port_method {
        return false;
    }
    if node.sources.is_empty() {
        return false;
    }
    if rvs_is_generated_snafu_helper(def_path) {
        return false;
    }
    local_crate_names
        .iter()
        .any(|name| def_path.rvs_starts_with(&name.rvs_prefix()))
}

fn rvs_is_generated_snafu_helper(def_path: &DefPath) -> bool {
    let path = def_path.rvs_as_str();
    let fn_name = def_path.rvs_fn_name();
    matches!(fn_name.rvs_as_str(), "build" | "fail")
        && path.split("::").any(|segment| segment.ends_with("Snafu"))
}

fn rvs_is_test_harness_callee(callee: &DefPath) -> bool {
    callee.rvs_as_str() == "test::test_main_static"
}

fn rvs_declared_caps(def_path: &DefPath) -> Option<CapabilitySet> {
    let fn_name = def_path.rvs_fn_name();
    let raw_suffix = capability::rvs_extract_raw_suffix(fn_name.rvs_as_str());
    let has_unknown_suffix = raw_suffix
        .chars()
        .any(|letter| Capability::rvs_from_char(letter).is_none());
    let (_, caps) = capability::rvs_parse_function(fn_name.rvs_as_str())?;
    if has_unknown_suffix && caps.rvs_is_empty() {
        return None;
    }
    Some(caps)
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
}
