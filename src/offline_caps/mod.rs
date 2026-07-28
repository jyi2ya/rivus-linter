use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::artifacts::{CallSiteIdentity, FnGraph, FnNode, FunctionIdentity};
use crate::callgraph_cache::rvs_is_std_like_def_path;
use crate::capability::{
    Capability, CapabilityCompleteness, CapabilityPolicy, CapabilitySet, ParsedFunctionName,
};
use crate::capsmap::CapsMap;
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::inference::{
    CallContractMismatchKind, CalleeCapsResolver, FnContractDiff, FnContractMismatchKind,
    PreparedLocalAnalysis, TraitImplOutlier, rvs_collect_call_contract_mismatch,
    rvs_contract_diff_for_expected_caps,
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
    IncompleteCapsKnowledge,
    NonAlphabeticalSuffix,
    StaticRefRequiresCaps,
    TraitImplOutlier,
    UnknownCallee,
    UnknownSuffixLetter,
}

impl OfflineCapsKind {
    pub(crate) fn rvs_as_str(self) -> &'static str {
        match self {
            Self::CallViolation => "call_violation",
            Self::Contract(kind) => kind.rvs_as_str(),
            Self::DuplicateSuffix => "duplicate_suffix",
            Self::IncompleteCapsKnowledge => "incomplete_caps_knowledge",
            Self::NonAlphabeticalSuffix => "non_alphabetical_suffix",
            Self::StaticRefRequiresCaps => "static_ref_requires_caps",
            Self::TraitImplOutlier => "trait_impl_outlier",
            Self::UnknownCallee => "unknown_callee",
            Self::UnknownSuffixLetter => "unknown_suffix_letter",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OfflineCapsLint {
    CallViolation,
    ContractMismatch,
    DuplicateSuffix,
    IncompleteCapsKnowledge,
    MissingAsync,
    MissingMutable,
    MissingRvsPrefix,
    MissingSideEffect,
    MissingThreadLocal,
    MissingUnsafe,
    NonAlphabeticalSuffix,
    StaticRef,
    TraitImplOutlier,
    UnknownCallee,
    UnknownSuffixLetter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OfflineCapsEmission {
    pub(crate) lint: OfflineCapsLint,
    pub(crate) span_anchors: BTreeSet<OfflineCapsEmissionAnchor>,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct OfflineCapsEmissionAnchor {
    pub(crate) identity: FunctionIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call_site: Option<CallSiteIdentity>,
    pub(crate) expectation_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OfflineCapsCallAnchor {
    caller: FunctionIdentity,
    call_site: CallSiteIdentity,
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
    pub(crate) span_anchors: BTreeMap<DefPath, BTreeSet<u64>>,
    call_site_anchors: BTreeSet<OfflineCapsCallAnchor>,
    pub(crate) message: String,
    pub(crate) details: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OfflineCapsReport {
    pub(crate) diagnostics: Vec<OfflineCapsDiagnostic>,
}

impl OfflineCapsReport {
    #[cfg(test)]
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

    pub(crate) fn rvs_emissions(&self, graph: &FnGraph) -> Vec<OfflineCapsEmission> {
        self.diagnostics
            .iter()
            .map(|diagnostic| {
                let mut actual_identities: BTreeSet<FunctionIdentity> = diagnostic
                    .span_anchors
                    .iter()
                    .flat_map(|(def_path, crate_ids)| {
                        crate_ids.iter().map(|crate_id| FunctionIdentity {
                            crate_id: *crate_id,
                            def_path: def_path.clone(),
                        })
                    })
                    .collect();
                actual_identities.extend(
                    diagnostic
                        .call_site_anchors
                        .iter()
                        .map(|anchor| anchor.caller.clone()),
                );
                let mut span_anchors: BTreeSet<OfflineCapsEmissionAnchor> = actual_identities
                    .iter()
                    .cloned()
                    .map(|identity| OfflineCapsEmissionAnchor {
                        identity,
                        call_site: None,
                        expectation_only: false,
                    })
                    .collect();
                if !diagnostic.call_site_anchors.is_empty() {
                    span_anchors = diagnostic
                        .call_site_anchors
                        .iter()
                        .cloned()
                        .map(|anchor| OfflineCapsEmissionAnchor {
                            identity: anchor.caller,
                            call_site: Some(anchor.call_site),
                            expectation_only: false,
                        })
                        .collect();
                }
                for identity in &actual_identities {
                    let Some(node) = graph.rvs_get(identity.def_path.rvs_as_str()) else {
                        continue;
                    };
                    if !node.production_crate_ids.contains(&identity.crate_id) {
                        continue;
                    }
                    for alias in rvs_test_compilation_aliases(node, identity) {
                        if actual_identities.contains(&alias) {
                            continue;
                        }
                        if diagnostic.call_site_anchors.is_empty() {
                            span_anchors.insert(OfflineCapsEmissionAnchor {
                                identity: alias,
                                call_site: None,
                                expectation_only: true,
                            });
                            continue;
                        }
                        for call_anchor in diagnostic
                            .call_site_anchors
                            .iter()
                            .filter(|anchor| anchor.caller == *identity)
                        {
                            let mut alias_call_sites = node
                                .coverage_call_sites
                                .get(&alias.crate_id)
                                .into_iter()
                                .flatten()
                                .filter(|candidate| {
                                    rvs_call_sites_are_source_aliases(
                                        &call_anchor.call_site,
                                        candidate,
                                    )
                                });
                            let Some(alias_call_site) = alias_call_sites.next().cloned() else {
                                continue;
                            };
                            if alias_call_sites.next().is_some() {
                                continue;
                            }
                            span_anchors.insert(OfflineCapsEmissionAnchor {
                                identity: alias.clone(),
                                call_site: Some(alias_call_site),
                                expectation_only: true,
                            });
                        }
                    }
                }
                OfflineCapsEmission {
                    lint: rvs_lint_for_kind(diagnostic.kind),
                    span_anchors,
                    message: if diagnostic.details.is_empty() {
                        diagnostic.message.clone()
                    } else {
                        format!("{}; {}", diagnostic.message, diagnostic.details.join("; "))
                    },
                }
            })
            .collect()
    }
}

pub(crate) fn rvs_serialize_emissions(emissions: &[OfflineCapsEmission]) -> Result<String, String> {
    rvs_validate_emissions(emissions)?;
    serde_json::to_string(emissions)
        .map_err(|error| format!("cannot serialize offline caps emissions: {error}"))
}

pub(crate) fn rvs_parse_emissions(json: &str) -> Result<Vec<OfflineCapsEmission>, String> {
    let emissions: Vec<OfflineCapsEmission> = serde_json::from_str(json)
        .map_err(|error| format!("cannot parse offline caps emissions: {error}"))?;
    rvs_validate_emissions(&emissions)?;
    Ok(emissions)
}

fn rvs_validate_emissions(emissions: &[OfflineCapsEmission]) -> Result<(), String> {
    for (index, emission) in emissions.iter().enumerate() {
        if emission.span_anchors.is_empty() {
            return Err(format!(
                "offline caps emission {index} must contain at least one diagnostic anchor"
            ));
        }
        for anchor in &emission.span_anchors {
            if anchor.identity.crate_id == 0 {
                return Err(format!(
                    "offline caps emission {index} contains a zero crate id anchor"
                ));
            }
            if anchor.identity.def_path.rvs_as_str().is_empty() {
                return Err(format!(
                    "offline caps emission {index} contains an empty function path anchor"
                ));
            }
            if let Some(call_site) = &anchor.call_site
                && (call_site.callee.crate_id == 0
                    || call_site.callee.def_path.rvs_as_str().is_empty())
            {
                return Err(format!(
                    "offline caps emission {index} contains an invalid call-site anchor"
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn rvs_emission_ack_name(emission_index: usize, anchor_index: usize) -> String {
    debug_assert!(
        emission_index < usize::MAX,
        "emission index is representable"
    );
    debug_assert!(anchor_index < usize::MAX, "anchor index is representable");
    format!("emission-{emission_index}-anchor-{anchor_index}.ack")
}

fn rvs_lint_for_kind(kind: OfflineCapsKind) -> OfflineCapsLint {
    match kind {
        OfflineCapsKind::CallViolation => OfflineCapsLint::CallViolation,
        OfflineCapsKind::Contract(kind) => match kind {
            FnContractMismatchKind::MissingAsync => OfflineCapsLint::MissingAsync,
            FnContractMismatchKind::MissingBlocking
            | FnContractMismatchKind::MissingIo
            | FnContractMismatchKind::MissingPort
            | FnContractMismatchKind::NameMismatch => OfflineCapsLint::ContractMismatch,
            FnContractMismatchKind::MissingMutable => OfflineCapsLint::MissingMutable,
            FnContractMismatchKind::MissingRvsPrefix => OfflineCapsLint::MissingRvsPrefix,
            FnContractMismatchKind::MissingSideEffect => OfflineCapsLint::MissingSideEffect,
            FnContractMismatchKind::MissingThreadLocal => OfflineCapsLint::MissingThreadLocal,
            FnContractMismatchKind::MissingUnsafe => OfflineCapsLint::MissingUnsafe,
        },
        OfflineCapsKind::DuplicateSuffix => OfflineCapsLint::DuplicateSuffix,
        OfflineCapsKind::IncompleteCapsKnowledge => OfflineCapsLint::IncompleteCapsKnowledge,
        OfflineCapsKind::NonAlphabeticalSuffix => OfflineCapsLint::NonAlphabeticalSuffix,
        OfflineCapsKind::StaticRefRequiresCaps => OfflineCapsLint::StaticRef,
        OfflineCapsKind::TraitImplOutlier => OfflineCapsLint::TraitImplOutlier,
        OfflineCapsKind::UnknownCallee => OfflineCapsLint::UnknownCallee,
        OfflineCapsKind::UnknownSuffixLetter => OfflineCapsLint::UnknownSuffixLetter,
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
    diagnostic_crate_ids: BTreeSet<u64>,
}

#[derive(Debug)]
struct IncompleteCapsUsage {
    layer: String,
    file: String,
    completeness: CapabilityCompleteness,
    callers: BTreeMap<DefPath, BTreeSet<u64>>,
    callees: BTreeMap<DefPath, String>,
}

#[derive(Debug)]
pub(crate) struct TargetTraitImplOutlierGroup {
    pub(crate) outlier: TraitImplOutlier,
    pub(crate) crate_ids: BTreeSet<u64>,
}

#[derive(Debug)]
struct TargetTraitContribution {
    implementation: DefPath,
    vote_caps: CapabilitySet,
    candidate_caps: Vec<(u64, CapabilitySet)>,
    incomplete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetRole {
    Production,
    Test,
}

#[derive(Debug)]
struct TargetInference {
    caps: BTreeMap<FunctionIdentity, CapabilitySet>,
    incomplete: BTreeSet<FunctionIdentity>,
}

type ContractDiagnosticGroups =
    BTreeMap<(FnContractMismatchKind, String, String, String), (FnContractDiff, BTreeSet<u64>)>;
type CallCapabilityMismatchGroups = BTreeMap<
    (String, String, String),
    (CapabilitySet, CapabilitySet, Vec<Capability>, BTreeSet<u64>),
>;
type StaticRefDiagnosticGroups =
    BTreeMap<(String, String), (CapabilitySet, Vec<Capability>, BTreeSet<u64>)>;

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
            span_anchors: BTreeMap::from([(
                self.def_path.clone(),
                self.rvs_preferred_diagnostic_crate_ids(),
            )]),
            call_site_anchors: BTreeSet::new(),
            message,
            details,
        }
    }

    fn rvs_call_diagnostic(
        &self,
        crate_ids: BTreeSet<u64>,
        callee: &DefPath,
        severity: OfflineCapsSeverity,
        kind: OfflineCapsKind,
        message: String,
        details: Vec<String>,
    ) -> OfflineCapsDiagnostic {
        let call_site_anchors = rvs_call_site_anchors(self.node, self.def_path, callee, &crate_ids);
        OfflineCapsDiagnostic {
            severity,
            kind,
            function: self.def_path.clone(),
            span_anchors: if call_site_anchors.is_empty() {
                BTreeMap::from([(self.def_path.clone(), crate_ids)])
            } else {
                BTreeMap::new()
            },
            call_site_anchors,
            message,
            details,
        }
    }

    fn rvs_preferred_diagnostic_crate_ids(&self) -> BTreeSet<u64> {
        rvs_preferred_diagnostic_crate_ids(self.node, self.diagnostic_crate_ids.clone())
    }
}

fn rvs_diagnostic_crate_ids(node: &FnNode) -> BTreeSet<u64> {
    let all = rvs_all_diagnostic_crate_ids(node);
    rvs_preferred_diagnostic_crate_ids(node, all)
}

fn rvs_all_diagnostic_crate_ids(node: &FnNode) -> BTreeSet<u64> {
    let mut crate_ids = node.production_crate_ids.clone();
    crate_ids.extend(node.test_crate_ids.iter().copied());
    crate_ids.extend(node.coverage_candidate_crate_ids.iter().copied());
    crate_ids.extend(node.sources_by_crate.keys().copied());
    crate_ids.extend(node.facts_by_crate.keys().copied());
    crate_ids.extend(node.has_body_by_crate.keys().copied());
    crate_ids.extend(node.coverage_calls.keys().copied());
    crate_ids.extend(node.entrypoint_crate_ids.iter().copied());
    crate_ids
}

fn rvs_target_has_body(node: &FnNode, crate_id: u64) -> bool {
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    node.has_body_by_crate
        .get(&crate_id)
        .copied()
        .unwrap_or(node.has_body)
}

fn rvs_target_role(node: &FnNode, crate_id: u64) -> TargetRole {
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    if node.production_crate_ids.contains(&crate_id) {
        TargetRole::Production
    } else {
        TargetRole::Test
    }
}

fn rvs_preferred_diagnostic_crate_ids(
    node: &FnNode,
    mut crate_ids: BTreeSet<u64>,
) -> BTreeSet<u64> {
    for test_crate_id in crate_ids.clone() {
        if node.production_crate_ids.contains(&test_crate_id) {
            continue;
        }
        let is_duplicate = node.production_crate_ids.iter().any(|production_crate_id| {
            crate_ids.contains(production_crate_id)
                && rvs_crate_sources_are_aliases(node, test_crate_id, *production_crate_id)
        });
        if is_duplicate {
            crate_ids.remove(&test_crate_id);
        }
    }
    crate_ids
}

fn rvs_crate_sources_are_aliases(node: &FnNode, crate_id: u64, production_crate_id: u64) -> bool {
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    debug_assert!(production_crate_id > 0, "stable crate id is nonzero");
    let sources = node.sources_by_crate.get(&crate_id);
    let production_sources = node.sources_by_crate.get(&production_crate_id);
    match (sources, production_sources) {
        (Some(sources), Some(production_sources)) => {
            (!sources.is_empty() && !sources.is_disjoint(production_sources))
                || (sources.is_empty()
                    && production_sources.is_empty()
                    && node.production_crate_ids.len() == 1)
        }
        _ => false,
    }
}

fn rvs_calling_crate_ids(node: &FnNode, callee: &DefPath) -> BTreeSet<u64> {
    let crate_ids: BTreeSet<u64> = node
        .coverage_calls
        .iter()
        .filter(|(_, calls)| calls.iter().any(|call| call.def_path == *callee))
        .map(|(crate_id, _)| *crate_id)
        .collect();
    if crate_ids.is_empty() {
        rvs_diagnostic_crate_ids(node)
    } else {
        crate_ids
    }
}

fn rvs_call_site_anchors(
    node: &FnNode,
    caller: &DefPath,
    callee: &DefPath,
    crate_ids: &BTreeSet<u64>,
) -> BTreeSet<OfflineCapsCallAnchor> {
    crate_ids
        .iter()
        .flat_map(|crate_id| {
            node.coverage_call_sites
                .get(crate_id)
                .into_iter()
                .flatten()
                .filter(|call_site| call_site.callee.def_path == *callee)
                .cloned()
                .map(|call_site| OfflineCapsCallAnchor {
                    caller: FunctionIdentity {
                        crate_id: *crate_id,
                        def_path: caller.clone(),
                    },
                    call_site,
                })
        })
        .collect()
}

fn rvs_call_sites_are_source_aliases(
    production: &CallSiteIdentity,
    candidate: &CallSiteIdentity,
) -> bool {
    if production.callee.def_path != candidate.callee.def_path {
        return false;
    }
    match (&production.source, &candidate.source) {
        (Some(production), Some(candidate)) => production == candidate,
        (None, None) => production.occurrence == candidate.occurrence,
        _ => false,
    }
}

fn rvs_infer_target_caps(graph: &FnGraph, resolver: &CalleeCapsResolver<'_>) -> TargetInference {
    let mut initial = BTreeMap::new();
    let mut bodyless_caps = BTreeMap::new();
    for (def_path, node) in graph.rvs_iter() {
        for crate_id in rvs_all_diagnostic_crate_ids(node) {
            let identity = FunctionIdentity {
                crate_id,
                def_path: def_path.clone(),
            };
            let caps = if node.facts.is_port_method {
                CapabilityPolicy::rvs_port_method_caps()
            } else if let Some(caps) = resolver.rvs_exact_caps(def_path) {
                caps
            } else {
                CapabilityPolicy::rvs_signature_caps(
                    node.facts_by_crate
                        .get(&crate_id)
                        .copied()
                        .unwrap_or(node.facts),
                )
            };
            if !rvs_target_has_body(node, crate_id)
                && !node.facts.is_port_method
                && resolver.rvs_exact_caps(def_path).is_none()
            {
                bodyless_caps.insert(identity.clone(), caps.clone());
            }
            initial.insert(identity, caps);
        }
    }

    let caps = loop {
        let mut inferred = initial.clone();
        for (identity, caps) in &bodyless_caps {
            inferred.insert(identity.clone(), caps.clone());
        }
        rvs_propagate_target_caps_M(graph, resolver, &mut inferred);

        let mut next_bodyless = bodyless_caps.clone();
        for identity in bodyless_caps.keys() {
            let caps = rvs_target_trait_vote_caps(graph, &inferred, identity)
                .or_else(|| resolver.rvs_for_propagation_target(&identity.def_path));
            if let Some(caps) = caps {
                next_bodyless.insert(identity.clone(), caps);
            }
        }
        if next_bodyless == bodyless_caps {
            break inferred;
        }
        bodyless_caps = next_bodyless;
    };
    let incomplete = rvs_infer_target_incomplete(graph, resolver, &caps);
    TargetInference { caps, incomplete }
}

fn rvs_propagate_target_caps_M(
    graph: &FnGraph,
    resolver: &CalleeCapsResolver<'_>,
    inferred: &mut BTreeMap<FunctionIdentity, CapabilitySet>,
) {
    loop {
        let mut changed = false;
        for (def_path, node) in graph.rvs_iter() {
            if node.facts.is_port_method || resolver.rvs_exact_caps(def_path).is_some() {
                continue;
            }
            for crate_id in rvs_all_diagnostic_crate_ids(node) {
                if !rvs_target_has_body(node, crate_id) {
                    continue;
                }
                let identity = FunctionIdentity {
                    crate_id,
                    def_path: def_path.clone(),
                };
                let mut combined = inferred
                    .get(&identity)
                    .cloned()
                    .unwrap_or_else(CapabilitySet::rvs_new);
                if let Some(calls) = node.coverage_calls.get(&crate_id) {
                    for call in calls {
                        let callee_caps = inferred
                            .get(call)
                            .cloned()
                            .or_else(|| resolver.rvs_for_propagation_target(&call.def_path));
                        if let Some(callee_caps) = callee_caps {
                            changed |= combined.rvs_extend_filtered_M(
                                &callee_caps,
                                CapabilityPolicy::rvs_is_propagated_cap,
                            );
                        }
                    }
                } else {
                    for callee in &node.calls {
                        if let Some(callee_caps) = resolver.rvs_for_propagation_target(callee) {
                            changed |= combined.rvs_extend_filtered_M(
                                &callee_caps,
                                CapabilityPolicy::rvs_is_propagated_cap,
                            );
                        }
                    }
                }
                inferred.insert(identity, combined);
            }
        }
        if !changed {
            return;
        }
    }
}

fn rvs_target_trait_vote_caps(
    graph: &FnGraph,
    inferred: &BTreeMap<FunctionIdentity, CapabilitySet>,
    trait_method: &FunctionIdentity,
) -> Option<CapabilitySet> {
    let trait_node = graph.rvs_get(trait_method.def_path.rvs_as_str())?;
    let role = rvs_target_role(trait_node, trait_method.crate_id);
    let implementations: Vec<CapabilitySet> = graph
        .rvs_iter()
        .filter(|(path, node)| {
            node.is_trait_impl
                && path.rvs_trait_method_identity().is_some_and(|identity| {
                    identity.rvs_trait_method_path() == trait_method.def_path
                })
        })
        .filter_map(|(path, node)| {
            let mut propagated = CapabilitySet::rvs_new();
            let mut found = false;
            for crate_id in rvs_trait_vote_crate_ids(node, role) {
                let identity = FunctionIdentity {
                    crate_id,
                    def_path: path.clone(),
                };
                if let Some(caps) = inferred.get(&identity) {
                    found = true;
                    let _ = propagated
                        .rvs_extend_filtered_M(caps, CapabilityPolicy::rvs_is_propagated_cap);
                }
            }
            found.then_some(propagated)
        })
        .collect();
    if implementations.is_empty() {
        return None;
    }
    let threshold = implementations.len().div_ceil(2);
    let mut selected = CapabilitySet::rvs_new();
    for capability in [
        Capability::B,
        Capability::I,
        Capability::P,
        Capability::S,
        Capability::T,
    ] {
        let count = implementations
            .iter()
            .filter(|caps| caps.rvs_contains(capability))
            .count();
        if count >= threshold {
            selected.rvs_insert_M(capability);
        }
    }
    Some(selected)
}

fn rvs_requested_role_crate_ids(node: &FnNode, role: TargetRole) -> Vec<u64> {
    rvs_all_diagnostic_crate_ids(node)
        .into_iter()
        .filter(|crate_id| {
            rvs_target_role(node, *crate_id) == role && rvs_target_has_body(node, *crate_id)
        })
        .collect()
}

fn rvs_trait_vote_crate_ids(node: &FnNode, role: TargetRole) -> Vec<u64> {
    let requested = rvs_requested_role_crate_ids(node, role);
    if requested.is_empty() && role == TargetRole::Test {
        rvs_requested_role_crate_ids(node, TargetRole::Production)
    } else {
        requested
    }
}

fn rvs_infer_target_incomplete(
    graph: &FnGraph,
    resolver: &CalleeCapsResolver<'_>,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
) -> BTreeSet<FunctionIdentity> {
    let mut incomplete = BTreeSet::new();
    for (path, node) in graph.rvs_iter() {
        for crate_id in rvs_all_diagnostic_crate_ids(node) {
            let identity = FunctionIdentity {
                crate_id,
                def_path: path.clone(),
            };
            if resolver.rvs_incomplete_exact_caps_info(path).is_some() {
                incomplete.insert(identity.clone());
            }
            if !rvs_target_has_body(node, crate_id) {
                continue;
            }
            let calls = node.coverage_calls.get(&crate_id);
            let has_unknown = calls.is_some_and(|calls| {
                calls.iter().any(|call| {
                    resolver
                        .rvs_incomplete_exact_caps_info(&call.def_path)
                        .is_some()
                        || (graph.rvs_get(call.def_path.rvs_as_str()).is_some()
                            && !target_caps.contains_key(call))
                        || (graph.rvs_get(call.def_path.rvs_as_str()).is_none()
                            && resolver.rvs_for_contract_check(&call.def_path).is_none())
                })
            });
            if has_unknown {
                incomplete.insert(identity);
            }
        }
    }

    loop {
        let mut newly_incomplete = BTreeSet::new();
        for (path, node) in graph.rvs_iter() {
            for crate_id in rvs_all_diagnostic_crate_ids(node) {
                let identity = FunctionIdentity {
                    crate_id,
                    def_path: path.clone(),
                };
                if incomplete.contains(&identity) {
                    continue;
                }
                let call_is_incomplete = node
                    .coverage_calls
                    .get(&crate_id)
                    .is_some_and(|calls| calls.iter().any(|call| incomplete.contains(call)));
                let vote_is_incomplete = !rvs_target_has_body(node, crate_id)
                    && rvs_target_trait_implementation_identities(graph, &identity)
                        .iter()
                        .any(|implementation| incomplete.contains(implementation));
                if call_is_incomplete || vote_is_incomplete {
                    newly_incomplete.insert(identity);
                }
            }
        }
        if newly_incomplete.is_empty() {
            return incomplete;
        }
        incomplete.extend(newly_incomplete);
    }
}

fn rvs_target_trait_implementation_identities(
    graph: &FnGraph,
    trait_method: &FunctionIdentity,
) -> BTreeSet<FunctionIdentity> {
    let Some(trait_node) = graph.rvs_get(trait_method.def_path.rvs_as_str()) else {
        return BTreeSet::new();
    };
    let role = rvs_target_role(trait_node, trait_method.crate_id);
    graph
        .rvs_iter()
        .filter(|(path, node)| {
            node.is_trait_impl
                && path.rvs_trait_method_identity().is_some_and(|identity| {
                    identity.rvs_trait_method_path() == trait_method.def_path
                })
        })
        .flat_map(|(path, node)| {
            rvs_trait_vote_crate_ids(node, role)
                .into_iter()
                .map(move |crate_id| FunctionIdentity {
                    crate_id,
                    def_path: path.clone(),
                })
        })
        .collect()
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
    let target_caps = rvs_infer_target_caps(&scoped_graph, &resolver);
    let mut unknown_callees: BTreeMap<String, BTreeMap<DefPath, BTreeSet<u64>>> = BTreeMap::new();
    let mut incomplete_caps: BTreeMap<String, IncompleteCapsUsage> = BTreeMap::new();
    for (def_path, node) in scoped_graph.rvs_iter() {
        let classification = FunctionClassification::rvs_new(&local_scope, def_path, node);
        let diagnostic_crate_ids: BTreeSet<u64> = rvs_all_diagnostic_crate_ids(node)
            .into_iter()
            .filter(|crate_id| {
                FunctionClassification::rvs_new_for_crate(&local_scope, def_path, node, *crate_id)
                    .rvs_is_offline_checked()
            })
            .collect();
        if diagnostic_crate_ids.is_empty() && !classification.rvs_is_offline_checked() {
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
            diagnostic_crate_ids,
        };
        rvs_collect_contract_diagnostics_M(
            &mut report,
            &context,
            &resolver,
            &target_caps.caps,
            &target_caps.incomplete,
        );
        rvs_collect_suffix_diagnostics_M(&mut report, &context);
        rvs_collect_static_ref_diagnostics_M(&mut report, &context);
        rvs_collect_call_diagnostics_M(
            &mut report,
            &context,
            &resolver,
            &target_caps.caps,
            &target_caps.incomplete,
            &mut unknown_callees,
            &mut incomplete_caps,
        );
    }
    let target_outliers = rvs_collect_target_trait_impl_outliers(
        &scoped_graph,
        &local_scope,
        &target_caps.caps,
        &target_caps.incomplete,
    );
    rvs_append_trait_impl_outliers_M(&mut report, &scoped_graph, &target_outliers);
    rvs_append_unknown_callee_diagnostics_M(&mut report, &scoped_graph, &unknown_callees);
    rvs_append_incomplete_caps_diagnostics_M(&mut report, &scoped_graph, &incomplete_caps);
    report.diagnostics.sort();
    report
}

fn rvs_append_trait_impl_outliers_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    outliers: &[TargetTraitImplOutlierGroup],
) {
    for group in outliers {
        let outlier = &group.outlier;
        let crate_ids = graph
            .rvs_get(outlier.implementation.rvs_as_str())
            .map(|_| group.crate_ids.clone())
            .unwrap_or_default();
        if crate_ids.is_empty() {
            continue;
        }
        let vote_counts = [
            Capability::B,
            Capability::I,
            Capability::P,
            Capability::S,
            Capability::T,
        ]
        .into_iter()
        .map(|capability| {
            format!(
                "{}={}/{}",
                capability.rvs_as_char(),
                outlier.counts.get(&capability).copied().unwrap_or(0),
                outlier.implementations
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::TraitImplOutlier,
            function: outlier.implementation.clone(),
            span_anchors: BTreeMap::from([(outlier.implementation.clone(), crate_ids)]),
            call_site_anchors: BTreeSet::new(),
            message: "trait implementation has capabilities outside the aggregate vote".to_string(),
            details: vec![
                format!("trait method: {}", outlier.trait_method),
                format!(
                    "implementation caps: {}",
                    rvs_format_caps(&outlier.implementation_caps)
                ),
                format!("voted caps: {}", rvs_format_caps(&outlier.selected_caps)),
                format!(
                    "outlier caps: {}",
                    rvs_format_caps(&outlier.unexpected_caps)
                ),
                format!(
                    "vote threshold: {}/{}",
                    outlier.threshold, outlier.implementations
                ),
                format!("votes: {vote_counts}"),
            ],
        });
    }
}

fn rvs_collect_target_trait_impl_outliers(
    graph: &FnGraph,
    local_scope: &LocalScope,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
    incomplete_identities: &BTreeSet<FunctionIdentity>,
) -> Vec<TargetTraitImplOutlierGroup> {
    let mut groups: Vec<TargetTraitImplOutlierGroup> = Vec::new();
    let trait_methods: BTreeSet<DefPath> = graph
        .rvs_iter()
        .filter(|(_, node)| node.is_trait_impl)
        .filter_map(|(path, _)| {
            path.rvs_trait_method_identity()
                .map(|identity| identity.rvs_trait_method_path())
        })
        .collect();
    for trait_method in trait_methods {
        if graph
            .rvs_get(trait_method.rvs_as_str())
            .is_some_and(|trait_node| trait_node.facts.is_port_method)
        {
            continue;
        }
        for role in [TargetRole::Production, TargetRole::Test] {
            let contributions = rvs_target_trait_contributions(
                graph,
                local_scope,
                target_caps,
                incomplete_identities,
                &trait_method,
                role,
            );
            if contributions.is_empty()
                || contributions
                    .iter()
                    .any(|contribution| contribution.incomplete)
            {
                continue;
            }
            let implementation_count = contributions.len();
            let threshold = implementation_count.div_ceil(2);
            let mut counts = BTreeMap::new();
            let mut selected_caps = CapabilitySet::rvs_new();
            for capability in [
                Capability::B,
                Capability::I,
                Capability::P,
                Capability::S,
                Capability::T,
            ] {
                let count = contributions
                    .iter()
                    .filter(|contribution| contribution.vote_caps.rvs_contains(capability))
                    .count();
                if count > 0 {
                    counts.insert(capability, count);
                }
                if count >= threshold {
                    selected_caps.rvs_insert_M(capability);
                }
            }
            for contribution in contributions {
                for (crate_id, implementation_caps) in contribution.candidate_caps {
                    let mut unexpected_caps = CapabilitySet::rvs_new();
                    let _ = unexpected_caps
                        .rvs_extend_filtered_M(&implementation_caps, |capability| {
                            !selected_caps.rvs_contains(capability)
                        });
                    if unexpected_caps.rvs_is_empty() {
                        continue;
                    }
                    let outlier = TraitImplOutlier {
                        trait_method: trait_method.clone(),
                        implementation: contribution.implementation.clone(),
                        implementation_caps,
                        selected_caps: selected_caps.clone(),
                        unexpected_caps,
                        implementations: implementation_count,
                        threshold,
                        counts: counts.clone(),
                    };
                    if let Some(group) = groups.iter_mut().find(|group| group.outlier == outlier) {
                        group.crate_ids.insert(crate_id);
                    } else {
                        groups.push(TargetTraitImplOutlierGroup {
                            outlier,
                            crate_ids: BTreeSet::from([crate_id]),
                        });
                    }
                }
            }
        }
    }
    groups.sort_by(|left, right| {
        left.outlier
            .implementation
            .cmp(&right.outlier.implementation)
            .then_with(|| left.crate_ids.cmp(&right.crate_ids))
    });
    groups
}

fn rvs_target_trait_contributions(
    graph: &FnGraph,
    local_scope: &LocalScope,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
    incomplete_identities: &BTreeSet<FunctionIdentity>,
    trait_method: &DefPath,
    role: TargetRole,
) -> Vec<TargetTraitContribution> {
    graph
        .rvs_iter()
        .filter(|(path, node)| {
            node.is_trait_impl
                && path
                    .rvs_trait_method_identity()
                    .is_some_and(|identity| identity.rvs_trait_method_path() == *trait_method)
        })
        .filter_map(|(implementation, node)| {
            let requested_ids = rvs_requested_role_crate_ids(node, role);
            let vote_ids = rvs_trait_vote_crate_ids(node, role);
            let mut vote_caps = CapabilitySet::rvs_new();
            let mut incomplete = false;
            let mut found = false;
            for crate_id in vote_ids {
                let identity = FunctionIdentity {
                    crate_id,
                    def_path: implementation.clone(),
                };
                incomplete |= incomplete_identities.contains(&identity);
                if let Some(caps) = target_caps.get(&identity) {
                    found = true;
                    let _ = vote_caps
                        .rvs_extend_filtered_M(caps, CapabilityPolicy::rvs_is_propagated_cap);
                }
            }
            if !found {
                return None;
            }
            let candidate_caps = requested_ids
                .into_iter()
                .filter(|crate_id| {
                    local_scope.rvs_contains(implementation)
                        && !node.facts.is_port_method
                        && node
                            .sources_by_crate
                            .get(crate_id)
                            .is_some_and(|sources| !sources.is_empty())
                })
                .filter_map(|crate_id| {
                    let identity = FunctionIdentity {
                        crate_id,
                        def_path: implementation.clone(),
                    };
                    target_caps.get(&identity).map(|caps| {
                        let mut propagated = CapabilitySet::rvs_new();
                        let _ = propagated
                            .rvs_extend_filtered_M(caps, CapabilityPolicy::rvs_is_propagated_cap);
                        (crate_id, propagated)
                    })
                })
                .collect();
            Some(TargetTraitContribution {
                implementation: implementation.clone(),
                vote_caps,
                candidate_caps,
                incomplete,
            })
        })
        .collect()
}

pub(crate) fn rvs_collect_report_trait_impl_outliers(
    graph: &FnGraph,
    caps: &CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
    analysis: &PreparedLocalAnalysis,
) -> Vec<TargetTraitImplOutlierGroup> {
    let resolver = analysis.rvs_resolver(graph, caps);
    let target_inference = rvs_infer_target_caps(graph, &resolver);
    let local_scope = LocalScope::rvs_new(local_crate_names);
    rvs_collect_target_trait_impl_outliers(
        graph,
        &local_scope,
        &target_inference.caps,
        &target_inference.incomplete,
    )
}

fn rvs_collect_contract_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    resolver: &CalleeCapsResolver<'_>,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
    incomplete_identities: &BTreeSet<FunctionIdentity>,
) {
    let mut groups = ContractDiagnosticGroups::new();
    for crate_id in context.diagnostic_crate_ids.iter().copied() {
        let expected = rvs_expected_caps_for_crate(
            context.def_path,
            context.node,
            crate_id,
            resolver,
            target_caps,
        );
        let diff = rvs_contract_diff_for_expected_caps(
            context.def_path,
            expected,
            incomplete_identities.contains(&FunctionIdentity {
                crate_id,
                def_path: context.def_path.clone(),
            }),
        );
        for kind in rvs_selected_contract_mismatch_kinds(&diff) {
            let key = (
                kind,
                diff.expected_name.to_string(),
                rvs_format_optional_caps(diff.declared_public_caps.as_ref()),
                diff.expected_public_caps.rvs_letters(),
            );
            groups
                .entry(key)
                .or_insert_with(|| (diff.clone(), BTreeSet::new()))
                .1
                .insert(crate_id);
        }
    }
    if context.diagnostic_crate_ids.is_empty()
        && let Some(diff) = context.contract_diff
    {
        for kind in rvs_selected_contract_mismatch_kinds(diff) {
            let key = (
                kind,
                diff.expected_name.to_string(),
                rvs_format_optional_caps(diff.declared_public_caps.as_ref()),
                diff.expected_public_caps.rvs_letters(),
            );
            groups.insert(
                key,
                (diff.clone(), context.rvs_preferred_diagnostic_crate_ids()),
            );
        }
    }

    for ((kind, _, _, _), (diff, crate_ids)) in groups {
        if crate_ids.is_empty() {
            continue;
        }
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
        let mut diagnostic = context.rvs_diagnostic(
            OfflineCapsSeverity::Warning,
            OfflineCapsKind::Contract(kind),
            message,
            details,
        );
        diagnostic.span_anchors = BTreeMap::from([(context.def_path.clone(), crate_ids)]);
        report.diagnostics.push(diagnostic);
    }
}

fn rvs_selected_contract_mismatch_kinds(diff: &FnContractDiff) -> Vec<FnContractMismatchKind> {
    let mismatch_kinds = diff.rvs_mismatch_kinds();
    if mismatch_kinds.contains(&FnContractMismatchKind::MissingRvsPrefix) {
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
    }
}

fn rvs_expected_caps_for_crate(
    def_path: &DefPath,
    node: &FnNode,
    crate_id: u64,
    resolver: &CalleeCapsResolver<'_>,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
) -> CapabilitySet {
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    let identity = FunctionIdentity {
        crate_id,
        def_path: def_path.clone(),
    };
    if let Some(caps) = target_caps.get(&identity) {
        return caps.clone();
    }
    if node.facts.is_port_method {
        return CapabilityPolicy::rvs_port_method_caps();
    }
    if let Some(caps) = resolver.rvs_exact_caps(def_path) {
        return caps;
    }
    if !rvs_target_has_body(node, crate_id)
        && let Some(caps) = resolver.rvs_for_contract_check(def_path)
    {
        return caps;
    }
    let facts = node
        .facts_by_crate
        .get(&crate_id)
        .copied()
        .unwrap_or(node.facts);
    let mut expected = CapabilityPolicy::rvs_signature_caps(facts);
    if let Some(calls) = node.coverage_calls.get(&crate_id) {
        for call in calls {
            if let Some(callee_caps) = resolver.rvs_for_contract_check(&call.def_path) {
                let _ = expected.rvs_extend_filtered_M(&callee_caps, |capability| {
                    CapabilityPolicy::rvs_is_propagated_cap(capability)
                });
            }
        }
    }
    expected
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
    node.sources_by_crate
        .iter()
        .filter(|(crate_id, _)| {
            !node.production_crate_ids.contains(crate_id)
                && rvs_crate_sources_are_aliases(node, **crate_id, production.crate_id)
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
    let mut groups: StaticRefDiagnosticGroups = BTreeMap::new();
    for crate_id in context.diagnostic_crate_ids.iter().copied() {
        let facts = context
            .node
            .facts_by_crate
            .get(&crate_id)
            .copied()
            .unwrap_or(context.node.facts);
        let required = rvs_required_static_caps(facts);
        let missing: Vec<_> = [Capability::S, Capability::T, Capability::U]
            .into_iter()
            .filter(|capability| {
                required.rvs_contains(*capability) && !declared.rvs_contains(*capability)
            })
            .collect();
        if !missing.is_empty() {
            let key = (required.rvs_letters(), rvs_format_cap_list(&missing));
            groups
                .entry(key)
                .or_insert_with(|| (required, missing, BTreeSet::new()))
                .2
                .insert(crate_id);
        }
    }
    if groups.is_empty() && context.diagnostic_crate_ids.is_empty() {
        let required = rvs_required_static_caps(context.node.facts);
        let missing: Vec<_> = [Capability::S, Capability::T, Capability::U]
            .into_iter()
            .filter(|capability| {
                required.rvs_contains(*capability) && !declared.rvs_contains(*capability)
            })
            .collect();
        if !missing.is_empty() {
            let key = (required.rvs_letters(), rvs_format_cap_list(&missing));
            groups.insert(
                key,
                (
                    required,
                    missing,
                    context.rvs_preferred_diagnostic_crate_ids(),
                ),
            );
        }
    }
    for (_, (required, missing, crate_ids)) in groups {
        if crate_ids.is_empty() {
            continue;
        }
        let mut diagnostic = context.rvs_diagnostic(
            OfflineCapsSeverity::Error,
            OfflineCapsKind::StaticRefRequiresCaps,
            "function touches static/thread-local state without declaring required caps"
                .to_string(),
            vec![
                format!("declared caps: {}", rvs_format_caps(declared)),
                format!(
                    "required caps from body facts: {}",
                    rvs_format_caps(&required)
                ),
                format!("missing: {}", rvs_format_cap_list(&missing)),
            ],
        );
        diagnostic.span_anchors = BTreeMap::from([(context.def_path.clone(), crate_ids)]);
        report.diagnostics.push(diagnostic);
    }
}

fn rvs_required_static_caps(facts: crate::capability::CapabilityFacts) -> CapabilitySet {
    let mut required = CapabilitySet::rvs_new();
    if facts.has_static_ref || facts.has_static_mut_ref || facts.has_thread_local_ref {
        required.rvs_insert_M(Capability::S);
    }
    if facts.has_static_mut_ref {
        required.rvs_insert_M(Capability::U);
    }
    if facts.has_thread_local_ref {
        required.rvs_insert_M(Capability::T);
    }
    required
}

fn rvs_collect_call_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    resolver: &CalleeCapsResolver<'_>,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
    incomplete_identities: &BTreeSet<FunctionIdentity>,
    unknown_callees: &mut BTreeMap<String, BTreeMap<DefPath, BTreeSet<u64>>>,
    incomplete_caps: &mut BTreeMap<String, IncompleteCapsUsage>,
) {
    if !context
        .diagnostic_crate_ids
        .iter()
        .copied()
        .any(|crate_id| rvs_target_has_body(context.node, crate_id))
    {
        return;
    }
    for callee in &context.node.calls {
        if rvs_is_test_harness_callee(callee) {
            continue;
        }
        if let Some(info) = resolver.rvs_incomplete_exact_caps_info(callee) {
            let source = info.rvs_source();
            let layer = source.map_or("<in-memory>", |source| source.layer.as_str());
            let file = source.map_or("<unknown>", |source| {
                source.file.to_str().unwrap_or("<non-utf8>")
            });
            let key = format!("{layer}\0{file}\0{}", info.rvs_completeness().rvs_name());
            let usage = incomplete_caps
                .entry(key)
                .or_insert_with(|| IncompleteCapsUsage {
                    layer: layer.to_string(),
                    file: file.to_string(),
                    completeness: info.rvs_completeness(),
                    callers: BTreeMap::new(),
                    callees: BTreeMap::new(),
                });
            usage
                .callers
                .entry(context.def_path.clone())
                .or_default()
                .extend(
                    rvs_calling_crate_ids(context.node, callee)
                        .intersection(&context.diagnostic_crate_ids)
                        .copied(),
                );
            usage.callees.insert(
                callee.clone(),
                format!(
                    "known caps: {}, basis={}",
                    rvs_format_caps(info.rvs_caps()),
                    info.rvs_basis().rvs_name()
                ),
            );
        } else {
            let incomplete_callers: BTreeSet<u64> = context
                .node
                .coverage_calls
                .iter()
                .filter(|(_, calls)| {
                    calls.iter().any(|call| {
                        call.def_path == *callee && incomplete_identities.contains(call)
                    })
                })
                .map(|(crate_id, _)| *crate_id)
                .filter(|crate_id| context.diagnostic_crate_ids.contains(crate_id))
                .collect();
            if !incomplete_callers.is_empty() {
                let usage = incomplete_caps
                    .entry("<inference>\0<callgraph>\0incomplete".to_string())
                    .or_insert_with(|| IncompleteCapsUsage {
                        layer: "<inference>".to_string(),
                        file: "<callgraph>".to_string(),
                        completeness: CapabilityCompleteness::Incomplete,
                        callers: BTreeMap::new(),
                        callees: BTreeMap::new(),
                    });
                usage
                    .callers
                    .entry(context.def_path.clone())
                    .or_default()
                    .extend(incomplete_callers);
                usage.callees.insert(
                    callee.clone(),
                    format!(
                        "known caps: {}, basis=inferred",
                        rvs_format_optional_caps(resolver.rvs_for_contract_check(callee).as_ref())
                    ),
                );
            }
        }
        let mut evaluations = Vec::new();
        for (crate_id, calls) in &context.node.coverage_calls {
            if !context.diagnostic_crate_ids.contains(crate_id) {
                continue;
            }
            let caller_identity = FunctionIdentity {
                crate_id: *crate_id,
                def_path: context.def_path.clone(),
            };
            let Some(caller_caps) = context
                .declared_caps
                .clone()
                .or_else(|| target_caps.get(&caller_identity).cloned())
                .or_else(|| context.inferred_caps.cloned())
            else {
                continue;
            };
            for call in calls.iter().filter(|call| call.def_path == *callee) {
                let callee_caps = rvs_target_contract_caps(call, target_caps, resolver);
                evaluations.push((*crate_id, caller_caps.clone(), callee_caps));
            }
        }
        if evaluations.is_empty() && context.node.coverage_calls.is_empty() {
            let Some(caller_caps) = context.declared_caps.clone().or_else(|| {
                context
                    .inferred_caps
                    .cloned()
                    .or_else(|| resolver.rvs_for_contract_check(context.def_path))
            }) else {
                continue;
            };
            let callee_caps = resolver.rvs_for_contract_check(callee);
            evaluations.extend(
                context
                    .rvs_preferred_diagnostic_crate_ids()
                    .into_iter()
                    .map(|crate_id| (crate_id, caller_caps.clone(), callee_caps.clone())),
            );
        }

        let mut unknown_ids = BTreeSet::new();
        let mut missing_groups = CallCapabilityMismatchGroups::new();
        for (crate_id, caller_caps, callee_caps) in evaluations {
            let Some(mismatch) = rvs_collect_call_contract_mismatch(
                callee.rvs_as_str(),
                &caller_caps,
                callee_caps.as_ref(),
            ) else {
                continue;
            };
            match mismatch.kind {
                CallContractMismatchKind::UnknownCallee => {
                    unknown_ids.insert(crate_id);
                }
                CallContractMismatchKind::MissingCapabilities => {
                    let callee_caps = mismatch
                        .callee_caps
                        .expect("never: missing-capability mismatch carries callee caps");
                    let missing: Vec<_> = mismatch.missing_caps.iter().copied().collect();
                    let key = (
                        caller_caps.rvs_letters(),
                        callee_caps.rvs_letters(),
                        rvs_format_cap_list(&missing),
                    );
                    missing_groups
                        .entry(key)
                        .or_insert_with(|| {
                            (caller_caps.clone(), callee_caps, missing, BTreeSet::new())
                        })
                        .3
                        .insert(crate_id);
                }
            }
        }
        if !unknown_ids.is_empty() {
            unknown_callees
                .entry(callee.to_string())
                .or_default()
                .entry(context.def_path.clone())
                .or_default()
                .extend(unknown_ids);
        }
        for (_, (caller_caps, callee_caps, missing, crate_ids)) in missing_groups {
            if crate_ids.is_empty() {
                continue;
            }
            report.diagnostics.push(context.rvs_call_diagnostic(
                crate_ids,
                callee,
                OfflineCapsSeverity::Error,
                OfflineCapsKind::CallViolation,
                "caller lacks propagated capabilities required by callee".to_string(),
                vec![
                    format!("callee: {callee}"),
                    format!("caller declared caps: {}", rvs_format_caps(&caller_caps)),
                    format!("callee caps: {}", rvs_format_caps(&callee_caps)),
                    format!("missing propagated caps: {}", rvs_format_cap_list(&missing)),
                ],
            ));
        }
    }
}

fn rvs_target_contract_caps(
    callee: &FunctionIdentity,
    target_caps: &BTreeMap<FunctionIdentity, CapabilitySet>,
    resolver: &CalleeCapsResolver<'_>,
) -> Option<CapabilitySet> {
    resolver
        .rvs_port_caps(&callee.def_path)
        .or_else(|| resolver.rvs_exact_caps(&callee.def_path))
        .or_else(|| ParsedFunctionName::rvs_parse(callee.def_path.rvs_as_str()).rvs_declared_caps())
        .or_else(|| target_caps.get(callee).cloned())
        .or_else(|| resolver.rvs_for_contract_check(&callee.def_path))
}

fn rvs_append_incomplete_caps_diagnostics_M(
    report: &mut OfflineCapsReport,
    _graph: &FnGraph,
    incomplete_caps: &BTreeMap<String, IncompleteCapsUsage>,
) {
    for usage in incomplete_caps.values() {
        let Some((first_caller, _)) = usage.callers.first_key_value() else {
            continue;
        };
        let mut details = vec![
            format!("layer: {}", usage.layer),
            format!("file: {}", usage.file),
            format!("completeness: {}", usage.completeness.rvs_name()),
            format!("affected callees: {}", usage.callees.len()),
        ];
        details.extend(
            usage
                .callees
                .iter()
                .take(5)
                .map(|(callee, knowledge)| format!("callee: {callee} ({knowledge})")),
        );
        if usage.callees.len() > 5 {
            details.push(format!(
                "... and {} more incomplete callees",
                usage.callees.len() - 5
            ));
        }
        details.push(if usage.layer == "std" {
            "run `cargo rivus infer-std -o caps/std` to replace migrated standard-library knowledge"
                .to_string()
        } else {
            format!(
                "refresh generated layer '{}' or add reviewed corrections to caps/ext",
                usage.layer
            )
        });
        details.push(format!("affected callers: {}", usage.callers.len()));
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::IncompleteCapsKnowledge,
            function: first_caller.clone(),
            span_anchors: usage.callers.clone(),
            call_site_anchors: BTreeSet::new(),
            message:
                "calls rely on incomplete caps knowledge; checks use known capability lower bounds"
                    .to_string(),
            details,
        });
    }
}

fn rvs_append_unknown_callee_diagnostics_M(
    report: &mut OfflineCapsReport,
    graph: &FnGraph,
    unknown_callees: &BTreeMap<String, BTreeMap<DefPath, BTreeSet<u64>>>,
) {
    for (callee, callers) in unknown_callees {
        let readable_callers: BTreeSet<String> = callers.keys().map(ToString::to_string).collect();
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
        let parsed = ParsedFunctionName::rvs_parse(callee);
        let missing_declaration = if parsed.rvs_has_rvs_prefix() {
            "has no valid capability declaration"
        } else {
            "has no rvs_ suffix"
        };
        let call_site_anchors =
            rvs_grouped_call_site_anchors(graph, callers, std::iter::once(&callee_path));
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::UnknownCallee,
            function: callee_path,
            span_anchors: if call_site_anchors.is_empty() {
                callers.clone()
            } else {
                BTreeMap::new()
            },
            call_site_anchors,
            message: format!("callee '{callee}' {missing_declaration} and no caps/ entry"),
            details,
        });
    }
}

fn rvs_grouped_call_site_anchors<'a>(
    graph: &FnGraph,
    callers: &BTreeMap<DefPath, BTreeSet<u64>>,
    callees: impl Iterator<Item = &'a DefPath>,
) -> BTreeSet<OfflineCapsCallAnchor> {
    let callees: BTreeSet<&DefPath> = callees.collect();
    let mut anchors = BTreeSet::new();
    for (caller, crate_ids) in callers {
        let Some(node) = graph.rvs_get(caller.rvs_as_str()) else {
            continue;
        };
        for callee in &callees {
            anchors.extend(rvs_call_site_anchors(node, caller, callee, crate_ids));
        }
    }
    anchors
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
    use crate::capability::{CapabilityFacts, CapabilityInfo, CapabilitySource};
    use crate::symbols::CapsMapKey;
    use crate::test_support::{rvs_make_capsmap, rvs_snapshot_BIS};
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
        node.coverage_call_sites.insert(
            1,
            calls
                .iter()
                .enumerate()
                .map(|(occurrence, call)| CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(*call),
                    },
                    occurrence: u32::try_from(occurrence)
                        .expect("never: test call count fits in u32"),
                    source: None,
                })
                .collect(),
        );
        node.production_crate_ids.insert(1);
        node.coverage_candidate_crate_ids.insert(1);
        node.sources_by_crate.insert(1, node.sources.clone());
        node
    }

    fn rvs_set_target_crates_M(node: &mut FnNode, crate_ids: &[u64]) {
        let crate_ids: BTreeSet<u64> = crate_ids.iter().copied().collect();
        node.production_crate_ids = crate_ids.clone();
        node.coverage_candidate_crate_ids = crate_ids.clone();
        node.facts_by_crate = crate_ids
            .iter()
            .map(|crate_id| (*crate_id, node.facts))
            .collect();
        node.has_body_by_crate = crate_ids
            .iter()
            .map(|crate_id| (*crate_id, node.has_body))
            .collect();
        node.coverage_calls = crate_ids
            .iter()
            .map(|crate_id| {
                let calls = node
                    .calls
                    .iter()
                    .map(|def_path| FunctionIdentity {
                        crate_id: *crate_id,
                        def_path: def_path.clone(),
                    })
                    .collect();
                (*crate_id, calls)
            })
            .collect();
        node.coverage_call_sites = crate_ids
            .iter()
            .map(|crate_id| {
                let call_sites = node
                    .calls
                    .iter()
                    .enumerate()
                    .map(|(occurrence, def_path)| CallSiteIdentity {
                        callee: FunctionIdentity {
                            crate_id: *crate_id,
                            def_path: def_path.clone(),
                        },
                        occurrence: u32::try_from(occurrence)
                            .expect("never: test call count fits in u32"),
                        source: None,
                    })
                    .collect();
                (*crate_id, call_sites)
            })
            .collect();
        node.sources_by_crate = crate_ids
            .iter()
            .map(|crate_id| (*crate_id, node.sources.clone()))
            .collect();
    }

    #[test]
    fn test_20260709_offline_caps_reports_call_violation_and_unknown() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["std::fs::read_to_string", "dep::plain"]),
        );
        let caps = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);
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
    fn test_20260715_offline_caps_reports_local_trait_impl_outlier() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dep::environment"]);
        outlier.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::EnvValue::rvs_parse@demo::FromString"),
            outlier,
        );
        let caps = rvs_make_capsmap(&[("dep::environment", "S")]);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260715_offline_caps_reports_local_trait_impl_outlier",
            &output,
        );

        assert!(!report.rvs_has_errors());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == OfflineCapsKind::TraitImplOutlier
                && diagnostic.function
                    == DefPath::from("demo::EnvValue::rvs_parse@demo::FromString")
        }));
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
    fn test_20260715_incomplete_caps_knowledge_is_not_treated_as_pure() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["dependency::maybe_effectful"]),
        );
        let mut info = CapabilityInfo::rvs_migrated_v1(
            CapabilitySet::rvs_new(),
            CapabilityCompleteness::Unknown,
        );
        info.rvs_with_source_M(CapabilitySource {
            layer: "deps".to_string(),
            file: PathBuf::from("caps/deps"),
            line: 2,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from("dependency::maybe_effectful"), info);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::IncompleteCapsKnowledge)
            .expect("never: incomplete caps knowledge emits a warning");
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260715_incomplete_caps_knowledge_is_not_treated_as_pure",
            &output,
        );

        assert!(diagnostic.message.contains("capability lower bounds"));
        assert!(output.contains("dependency::maybe_effectful"));
        assert!(output.contains("refresh generated layer 'deps'"));
    }

    #[test]
    fn test_20260715_in_memory_incomplete_trait_dispatch_is_reported() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::Parser::rvs_parse"), declaration);
        let mut implementation = rvs_node(&["dependency::unknown"]);
        implementation.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Adapter::rvs_parse@demo::Parser"),
            implementation,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_use_parser"),
            rvs_node(&["demo::Parser::rvs_parse"]),
        );

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260715_in_memory_incomplete_trait_dispatch_is_reported",
            &output,
        );

        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == OfflineCapsKind::IncompleteCapsKnowledge
                && diagnostic
                    .details
                    .iter()
                    .any(|detail| detail.contains("demo::Parser::rvs_parse"))
        }));
    }

    #[test]
    fn test_20260715_call_emission_is_scoped_to_violating_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_node(&["dependency::effect"]);
        caller.production_crate_ids = BTreeSet::from([10]);
        caller.coverage_candidate_crate_ids = BTreeSet::from([10]);
        caller.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 50,
                    def_path: DefPath::from("dependency::effect"),
                }]),
            ),
            (20, BTreeSet::new()),
        ]);
        caller.sources_by_crate =
            BTreeMap::from([(10, caller.sources.clone()), (20, caller.sources.clone())]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_call"), caller);
        let caps = rvs_make_capsmap(&[("dependency::effect", "S")]);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::CallViolation)
            .expect("never: call violation produces an emission");
        let output = format!(
            "anchors={}\n",
            emission
                .span_anchors
                .iter()
                .map(|anchor| {
                    format!(
                        "{}:{}:{}",
                        anchor.identity.crate_id, anchor.identity.def_path, anchor.expectation_only
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        rvs_snapshot_BIS(
            "test_20260715_call_emission_is_scoped_to_violating_crate_identity",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([
                OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("demo::rvs_call"),
                    },
                    call_site: None,
                    expectation_only: false,
                },
                OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 20,
                        def_path: DefPath::from("demo::rvs_call"),
                    },
                    call_site: None,
                    expectation_only: true,
                },
            ])
        );
    }

    #[test]
    fn test_20260715_non_call_emissions_are_scoped_to_violating_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut node = rvs_node(&["dependency::effect"]);
        node.facts = static_facts;
        node.facts_by_crate =
            BTreeMap::from([(10, static_facts), (20, CapabilityFacts::default())]);
        node.production_crate_ids = BTreeSet::from([10, 20]);
        node.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        node.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 50,
                    def_path: DefPath::from("dependency::effect"),
                }]),
            ),
            (20, BTreeSet::new()),
        ]);
        node.sources_by_crate =
            BTreeMap::from([(10, node.sources.clone()), (20, node.sources.clone())]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_read_cache"), node);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emissions = report.rvs_emissions(&graph);
        let mut output = String::new();
        for lint in [
            OfflineCapsLint::MissingSideEffect,
            OfflineCapsLint::StaticRef,
        ] {
            let emission = emissions
                .iter()
                .find(|emission| emission.lint == lint)
                .expect("never: target-specific behavior produces an emission");
            output.push_str(&format!("{lint:?}={:?}\n", emission.span_anchors));
            assert_eq!(
                emission.span_anchors,
                BTreeSet::from([OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("demo::rvs_read_cache"),
                    },
                    call_site: None,
                    expectation_only: false,
                }])
            );
        }
        rvs_snapshot_BIS(
            "test_20260715_non_call_emissions_are_scoped_to_violating_crate_identity",
            &output,
        );
    }

    #[test]
    fn test_20260715_local_callee_caps_are_scoped_to_target_identity() {
        let mut graph = FnGraph::rvs_new();
        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut callee = rvs_node(&[]);
        callee.facts = static_facts;
        callee.facts_by_crate =
            BTreeMap::from([(10, static_facts), (20, CapabilityFacts::default())]);
        callee.production_crate_ids = BTreeSet::from([10, 20]);
        callee.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        callee.coverage_calls = BTreeMap::from([(10, BTreeSet::new()), (20, BTreeSet::new())]);
        callee.sources_by_crate =
            BTreeMap::from([(10, callee.sources.clone()), (20, callee.sources.clone())]);
        let callee_path = DefPath::from("demo::effect");
        graph.rvs_insert_M(callee_path.clone(), callee);

        let mut caller = rvs_node(&["demo::effect"]);
        caller.production_crate_ids = BTreeSet::from([10, 20]);
        caller.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        caller.facts_by_crate = BTreeMap::from([
            (10, CapabilityFacts::default()),
            (20, CapabilityFacts::default()),
        ]);
        caller.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 10,
                    def_path: callee_path.clone(),
                }]),
            ),
            (
                20,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 20,
                    def_path: callee_path,
                }]),
            ),
        ]);
        caller.sources_by_crate =
            BTreeMap::from([(10, caller.sources.clone()), (20, caller.sources.clone())]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_call"), caller);

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::CallViolation)
            .expect("never: only the static-using callee target violates the call contract");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_local_callee_caps_are_scoped_to_target_identity",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 10,
                    def_path: DefPath::from("demo::rvs_call"),
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_port_static_ref_emission_keeps_target_anchor() {
        let mut graph = FnGraph::rvs_new();
        let facts = CapabilityFacts {
            is_port_method: true,
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut node = rvs_node(&[]);
        node.facts = facts;
        node.facts_by_crate.insert(1, facts);
        let path = DefPath::from("demo::ApiClient::rvs_fetch_P");
        graph.rvs_insert_M(path.clone(), node);

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::StaticRef)
            .expect("never: Port body static access still requires S");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_port_static_ref_emission_keeps_target_anchor",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 1,
                    def_path: path,
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_bodyless_trait_contract_keeps_vote_derived_anchor() {
        let mut graph = FnGraph::rvs_new();
        let declaration_path = DefPath::from("demo::Parser::parse");
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        graph.rvs_insert_M(declaration_path.clone(), declaration);
        for implementation in ["demo::First", "demo::Second"] {
            let mut node = rvs_node(&["dependency::effect"]);
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::parse@demo::Parser")),
                node,
            );
        }

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::MissingRvsPrefix)
            .expect("never: bodyless trait vote produces a contract diagnostic");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_bodyless_trait_contract_keeps_vote_derived_anchor",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 1,
                    def_path: declaration_path,
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_bodyless_trait_vote_is_scoped_to_target_identity() {
        let mut graph = FnGraph::rvs_new();
        let declaration_path = DefPath::from("demo::Parser::rvs_parse");
        let mut declaration = rvs_node(&[]);
        declaration.has_body = true;
        declaration.production_crate_ids = BTreeSet::from([10, 20]);
        declaration.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        declaration.facts_by_crate = BTreeMap::from([
            (10, CapabilityFacts::default()),
            (20, CapabilityFacts::default()),
        ]);
        declaration.has_body_by_crate = BTreeMap::from([(10, false), (20, true)]);
        declaration.coverage_calls = BTreeMap::from([(10, BTreeSet::new()), (20, BTreeSet::new())]);
        declaration.sources_by_crate = BTreeMap::from([
            (10, declaration.sources.clone()),
            (20, declaration.sources.clone()),
        ]);
        graph.rvs_insert_M(declaration_path.clone(), declaration);
        for implementation in ["demo::First", "demo::Second"] {
            let mut node = rvs_node(&["dependency::effect"]);
            node.is_trait_impl = true;
            node.production_crate_ids = BTreeSet::from([10, 20]);
            node.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
            node.facts_by_crate = BTreeMap::from([
                (10, CapabilityFacts::default()),
                (20, CapabilityFacts::default()),
            ]);
            node.has_body_by_crate = BTreeMap::from([(10, true), (20, true)]);
            node.coverage_calls = BTreeMap::from([
                (
                    10,
                    BTreeSet::from([FunctionIdentity {
                        crate_id: 50,
                        def_path: DefPath::from("dependency::effect"),
                    }]),
                ),
                (20, BTreeSet::new()),
            ]);
            node.sources_by_crate =
                BTreeMap::from([(10, node.sources.clone()), (20, node.sources.clone())]);
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                node,
            );
        }
        let mut caller = rvs_node(&["demo::Parser::rvs_parse"]);
        caller.production_crate_ids = BTreeSet::from([10, 20]);
        caller.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        caller.facts_by_crate = BTreeMap::from([
            (10, CapabilityFacts::default()),
            (20, CapabilityFacts::default()),
        ]);
        caller.has_body_by_crate = BTreeMap::from([(10, true), (20, true)]);
        caller.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 10,
                    def_path: declaration_path.clone(),
                }]),
            ),
            (
                20,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 20,
                    def_path: declaration_path,
                }]),
            ),
        ]);
        caller.sources_by_crate =
            BTreeMap::from([(10, caller.sources.clone()), (20, caller.sources.clone())]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_use_parser"), caller);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::MissingSideEffect)
            .expect("never: only the effectful trait-vote target requires an S marker");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_bodyless_trait_vote_is_scoped_to_target_identity",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 10,
                    def_path: DefPath::from("demo::Parser::rvs_parse"),
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_distinct_test_cfg_body_keeps_its_diagnostic_anchor() {
        let mut graph = FnGraph::rvs_new();
        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut node = rvs_node(&[]);
        node.facts = static_facts;
        node.facts_by_crate =
            BTreeMap::from([(10, CapabilityFacts::default()), (20, static_facts)]);
        node.production_crate_ids = BTreeSet::from([10]);
        node.test_crate_ids = BTreeSet::from([20]);
        node.coverage_candidate_crate_ids = BTreeSet::from([10]);
        node.coverage_calls = BTreeMap::from([(10, BTreeSet::new()), (20, BTreeSet::new())]);
        node.sources_by_crate = BTreeMap::from([
            (
                10,
                BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 1, 2)]),
            ),
            (
                20,
                BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 3, 4)]),
            ),
        ]);
        let path = DefPath::from("demo::rvs_read_cache");
        graph.rvs_insert_M(path.clone(), node);

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::StaticRef)
            .expect("never: distinct cfg(test) body retains its own diagnostic");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_distinct_test_cfg_body_keeps_its_diagnostic_anchor",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 20,
                    def_path: path,
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_same_source_test_only_behavior_keeps_diagnostic_anchors() {
        let path = DefPath::from("demo::rvs_variant");
        let mut production = rvs_node(&[]);
        production.production_crate_ids = BTreeSet::from([10]);
        production.coverage_candidate_crate_ids = BTreeSet::from([10]);
        production.test_crate_ids.clear();
        production.coverage_calls = BTreeMap::from([(10, BTreeSet::new())]);
        production.facts_by_crate = BTreeMap::from([(10, CapabilityFacts::default())]);
        production.sources_by_crate = BTreeMap::from([(10, production.sources.clone())]);
        let mut production_graph = FnGraph::rvs_new();
        production_graph.rvs_insert_M(path.clone(), production);

        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut test = rvs_node(&["dependency::effect"]);
        test.is_test_compilation = true;
        test.facts = static_facts;
        test.production_crate_ids.clear();
        test.coverage_candidate_crate_ids.clear();
        test.test_crate_ids = BTreeSet::from([20]);
        test.coverage_calls = BTreeMap::from([(
            20,
            BTreeSet::from([FunctionIdentity {
                crate_id: 50,
                def_path: DefPath::from("dependency::effect"),
            }]),
        )]);
        test.facts_by_crate = BTreeMap::from([(20, static_facts)]);
        test.sources_by_crate = BTreeMap::from([(20, test.sources.clone())]);
        let mut test_graph = FnGraph::rvs_new();
        test_graph.rvs_insert_M(path.clone(), test);

        let graph = FnGraph::rvs_merge_artifacts(
            vec![production_graph, test_graph],
            &BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emissions = report.rvs_emissions(&graph);
        let mut output = String::new();
        for lint in [OfflineCapsLint::CallViolation, OfflineCapsLint::StaticRef] {
            let emission = emissions
                .iter()
                .find(|emission| emission.lint == lint)
                .expect("never: test-only behavior remains represented after artifact merge");
            output.push_str(&format!("{lint:?}={:?}\n", emission.span_anchors));
            assert_eq!(
                emission.span_anchors,
                BTreeSet::from([OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 20,
                        def_path: path.clone(),
                    },
                    call_site: None,
                    expectation_only: false,
                }])
            );
        }
        rvs_snapshot_BIS(
            "test_20260715_same_source_test_only_behavior_keeps_diagnostic_anchors",
            &output,
        );
    }

    #[test]
    fn test_20260715_trait_outlier_emission_is_scoped_to_violating_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        rvs_set_target_crates_M(&mut declaration, &[10, 20]);
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            rvs_set_target_crates_M(&mut node, &[10, 20]);
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dependency::effect"]);
        outlier.is_trait_impl = true;
        outlier.facts_by_crate = BTreeMap::from([
            (10, CapabilityFacts::default()),
            (20, CapabilityFacts::default()),
        ]);
        outlier.production_crate_ids = BTreeSet::from([10, 20]);
        outlier.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        outlier.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 50,
                    def_path: DefPath::from("dependency::effect"),
                }]),
            ),
            (20, BTreeSet::new()),
        ]);
        outlier.sources_by_crate =
            BTreeMap::from([(10, outlier.sources.clone()), (20, outlier.sources.clone())]);
        let outlier_path = DefPath::from("demo::EnvValue::rvs_parse@demo::FromString");
        graph.rvs_insert_M(outlier_path.clone(), outlier);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::TraitImplOutlier)
            .expect("never: effectful implementation is a trait vote outlier");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260715_trait_outlier_emission_is_scoped_to_violating_crate_identity",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 10,
                    def_path: outlier_path,
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260715_trait_outlier_split_across_targets_keeps_both_anchors() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        rvs_set_target_crates_M(&mut declaration, &[10, 20]);
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            rvs_set_target_crates_M(&mut node, &[10, 20]);
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dependency::side_effect", "dependency::thread_local"]);
        outlier.is_trait_impl = true;
        outlier.facts_by_crate = BTreeMap::from([
            (10, CapabilityFacts::default()),
            (20, CapabilityFacts::default()),
        ]);
        outlier.production_crate_ids = BTreeSet::from([10, 20]);
        outlier.coverage_candidate_crate_ids = BTreeSet::from([10, 20]);
        outlier.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 50,
                    def_path: DefPath::from("dependency::side_effect"),
                }]),
            ),
            (
                20,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 60,
                    def_path: DefPath::from("dependency::thread_local"),
                }]),
            ),
        ]);
        outlier.sources_by_crate =
            BTreeMap::from([(10, outlier.sources.clone()), (20, outlier.sources.clone())]);
        let outlier_path = DefPath::from("demo::EnvValue::rvs_parse@demo::FromString");
        graph.rvs_insert_M(outlier_path.clone(), outlier);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[
                ("dependency::side_effect", "S"),
                ("dependency::thread_local", "T"),
            ]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let anchors: BTreeSet<OfflineCapsEmissionAnchor> = report
            .rvs_emissions(&graph)
            .into_iter()
            .filter(|emission| emission.lint == OfflineCapsLint::TraitImplOutlier)
            .flat_map(|emission| emission.span_anchors)
            .collect();
        let output = format!("anchors={anchors:?}\n");
        rvs_snapshot_BIS(
            "test_20260715_trait_outlier_split_across_targets_keeps_both_anchors",
            &output,
        );

        assert_eq!(anchors.len(), 2);
        assert!(anchors.iter().all(|anchor| {
            anchor.identity.def_path == outlier_path && !anchor.expectation_only
        }));
    }

    #[test]
    fn test_20260716_target_incompleteness_is_scoped_to_calling_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_node(&["dependency::unknown"]);
        rvs_set_target_crates_M(&mut node, &[10, 20]);
        node.coverage_calls = BTreeMap::from([
            (
                10,
                BTreeSet::from([FunctionIdentity {
                    crate_id: 50,
                    def_path: DefPath::from("dependency::unknown"),
                }]),
            ),
            (20, BTreeSet::new()),
        ]);
        node.coverage_call_sites = BTreeMap::from([(10, BTreeSet::new()), (20, BTreeSet::new())]);
        let path = DefPath::from("demo::rvs_handle_S");
        graph.rvs_insert_M(path.clone(), node);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &CapsMap::rvs_new(), &local);
        let empty_caps = CapsMap::rvs_new();
        let resolver = analysis.rvs_resolver(&scoped_graph, &empty_caps);
        let target_inference = rvs_infer_target_caps(&scoped_graph, &resolver);
        let incomplete: Vec<_> = target_inference.incomplete.iter().cloned().collect();
        let output = format!("incomplete={incomplete:?}\n");
        rvs_snapshot_BIS(
            "test_20260716_target_incompleteness_is_scoped_to_calling_identity",
            &output,
        );

        assert_eq!(
            target_inference.incomplete,
            BTreeSet::from([FunctionIdentity {
                crate_id: 10,
                def_path: path,
            }])
        );
    }

    #[test]
    fn test_20260716_trait_outlier_vote_combines_cross_crate_implementations() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        rvs_set_target_crates_M(&mut declaration, &[100]);
        graph.rvs_insert_M(DefPath::from("demo::Parser::rvs_parse"), declaration);
        for (implementation, crate_id) in [("demo::Alpha", 11), ("demo::Beta", 12)] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            rvs_set_target_crates_M(&mut node, &[crate_id]);
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dependency::effect"]);
        outlier.is_trait_impl = true;
        rvs_set_target_crates_M(&mut outlier, &[13]);
        let outlier_path = DefPath::from("demo::Gamma::rvs_parse@demo::Parser");
        graph.rvs_insert_M(outlier_path.clone(), outlier);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::TraitImplOutlier)
            .expect("never: cross-crate effectful implementation is an outlier");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS(
            "test_20260716_trait_outlier_vote_combines_cross_crate_implementations",
            &output,
        );

        assert_eq!(
            emission.span_anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 13,
                    def_path: outlier_path,
                },
                call_site: None,
                expectation_only: false,
            }])
        );
    }

    #[test]
    fn test_20260716_static_requirements_are_grouped_per_target() {
        let mut graph = FnGraph::rvs_new();
        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let thread_local_facts = CapabilityFacts {
            has_thread_local_ref: true,
            ..CapabilityFacts::default()
        };
        let mut node = rvs_node(&[]);
        rvs_set_target_crates_M(&mut node, &[10, 20]);
        node.facts = thread_local_facts;
        node.facts_by_crate = BTreeMap::from([(10, static_facts), (20, thread_local_facts)]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_read"), node);

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diagnostics: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::StaticRefRequiresCaps)
            .collect();
        let output = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "anchors={:?} details={}\n",
                    diagnostic.span_anchors,
                    diagnostic.details.join("; ")
                )
            })
            .collect::<String>();
        rvs_snapshot_BIS(
            "test_20260716_static_requirements_are_grouped_per_target",
            &output,
        );

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.span_anchors.values().flatten().copied().eq([10])
                && diagnostic
                    .details
                    .iter()
                    .any(|detail| detail == "missing: S")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.span_anchors.values().flatten().copied().eq([20])
                && diagnostic
                    .details
                    .iter()
                    .any(|detail| detail == "missing: S, T")
        }));
    }

    #[test]
    fn test_20260716_call_emission_uses_call_site_anchor() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_call"),
            rvs_node(&["dependency::effect"]),
        );
        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::CallViolation)
            .expect("never: pure caller violates the effectful callee contract");
        let output = format!("anchors={:?}\n", emission.span_anchors);
        rvs_snapshot_BIS("test_20260716_call_emission_uses_call_site_anchor", &output);

        assert!(emission.span_anchors.iter().all(|anchor| {
            anchor.call_site.as_ref().is_some_and(|call_site| {
                call_site.occurrence == 0
                    && call_site.callee.def_path.rvs_as_str() == "dependency::effect"
            })
        }));
    }

    #[test]
    fn test_20260716_call_site_emission_keeps_test_expectation_alias() {
        let caller = DefPath::from("demo::rvs_call");
        let callee = DefPath::from("dependency::effect");
        let mut node = rvs_node(&[callee.rvs_as_str()]);
        rvs_set_target_crates_M(&mut node, &[10, 20]);
        node.production_crate_ids = BTreeSet::from([10]);
        node.test_crate_ids = BTreeSet::from([20]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(caller.clone(), node.clone());
        let production_call_site = node
            .coverage_call_sites
            .get(&10)
            .and_then(|call_sites| call_sites.first())
            .cloned()
            .expect("never: production call site is present");
        let report = OfflineCapsReport {
            diagnostics: vec![OfflineCapsDiagnostic {
                severity: OfflineCapsSeverity::Error,
                kind: OfflineCapsKind::CallViolation,
                function: caller.clone(),
                span_anchors: BTreeMap::new(),
                call_site_anchors: BTreeSet::from([OfflineCapsCallAnchor {
                    caller: FunctionIdentity {
                        crate_id: 10,
                        def_path: caller,
                    },
                    call_site: production_call_site,
                }]),
                message: "violation".to_string(),
                details: Vec::new(),
            }],
        };

        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .next()
            .expect("never: report contains one emission");
        let output = emission
            .span_anchors
            .iter()
            .map(|anchor| {
                format!(
                    "crate={} occurrence={:?} expectation_only={}",
                    anchor.identity.crate_id,
                    anchor.call_site.as_ref().map(|site| site.occurrence),
                    anchor.expectation_only,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS(
            "test_20260716_call_site_emission_keeps_test_expectation_alias",
            &output,
        );

        assert!(emission.span_anchors.iter().any(|anchor| {
            anchor.identity.crate_id == 20 && anchor.call_site.is_some() && anchor.expectation_only
        }));
    }

    #[test]
    fn test_20260716_call_site_alias_matches_source_across_cfg_occurrence_shift() {
        let caller = DefPath::from("demo::rvs_call");
        let callee = DefPath::from("dependency::effect");
        let production_source =
            crate::artifacts::CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 30, 40);
        let test_only_source =
            crate::artifacts::CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 10, 20);
        let mut node = rvs_node(&[callee.rvs_as_str()]);
        rvs_set_target_crates_M(&mut node, &[10, 20]);
        node.production_crate_ids = BTreeSet::from([10]);
        node.test_crate_ids = BTreeSet::from([20]);
        node.coverage_call_sites.insert(
            10,
            BTreeSet::from([CallSiteIdentity {
                callee: FunctionIdentity {
                    crate_id: 100,
                    def_path: callee.clone(),
                },
                occurrence: 0,
                source: Some(production_source.clone()),
            }]),
        );
        node.coverage_call_sites.insert(
            20,
            BTreeSet::from([
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 200,
                        def_path: callee.clone(),
                    },
                    occurrence: 0,
                    source: Some(test_only_source),
                },
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 200,
                        def_path: callee,
                    },
                    occurrence: 1,
                    source: Some(production_source.clone()),
                },
            ]),
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(caller.clone(), node.clone());
        let report = OfflineCapsReport {
            diagnostics: vec![OfflineCapsDiagnostic {
                severity: OfflineCapsSeverity::Error,
                kind: OfflineCapsKind::CallViolation,
                function: caller.clone(),
                span_anchors: BTreeMap::new(),
                call_site_anchors: BTreeSet::from([OfflineCapsCallAnchor {
                    caller: FunctionIdentity {
                        crate_id: 10,
                        def_path: caller,
                    },
                    call_site: node
                        .coverage_call_sites
                        .get(&10)
                        .and_then(|sites| sites.first())
                        .cloned()
                        .expect("never: production call site exists"),
                }]),
                message: "violation".to_string(),
                details: Vec::new(),
            }],
        };

        let alias = report
            .rvs_emissions(&graph)
            .into_iter()
            .next()
            .expect("never: report contains one emission")
            .span_anchors
            .into_iter()
            .find(|anchor| anchor.expectation_only)
            .expect("never: test expectation alias exists");
        let output = format!(
            "occurrence={:?}\nsource_match={}\n",
            alias.call_site.as_ref().map(|site| site.occurrence),
            alias
                .call_site
                .as_ref()
                .and_then(|site| site.source.as_ref())
                == Some(&production_source),
        );
        rvs_snapshot_BIS(
            "test_20260716_call_site_alias_matches_source_across_cfg_occurrence_shift",
            &output,
        );

        assert_eq!(alias.call_site.map(|site| site.occurrence), Some(1));
    }

    #[test]
    fn test_20260716_test_trait_vote_falls_back_to_production_implementation() {
        let trait_method = DefPath::from("demo::Parser::rvs_parse");
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        rvs_set_target_crates_M(&mut declaration, &[10, 20]);
        declaration.production_crate_ids = BTreeSet::from([10]);
        declaration.test_crate_ids = BTreeSet::from([20]);
        declaration.has_body = false;
        declaration.has_body_by_crate = BTreeMap::from([(10, false), (20, false)]);
        graph.rvs_insert_M(trait_method.clone(), declaration);

        let first_impl_path = DefPath::from("demo::Alpha::rvs_parse@demo::Parser");
        let mut first_impl = rvs_node(&[]);
        first_impl.is_trait_impl = true;
        rvs_set_target_crates_M(&mut first_impl, &[20]);
        first_impl.production_crate_ids.clear();
        first_impl.test_crate_ids = BTreeSet::from([20]);
        graph.rvs_insert_M(first_impl_path.clone(), first_impl);

        let second_impl_path = DefPath::from("demo::Beta::rvs_parse@demo::Parser");
        let mut second_impl = rvs_node(&[]);
        second_impl.is_trait_impl = true;
        rvs_set_target_crates_M(&mut second_impl, &[30]);
        graph.rvs_insert_M(second_impl_path.clone(), second_impl);

        let inferred = BTreeMap::from([
            (
                FunctionIdentity {
                    crate_id: 20,
                    def_path: first_impl_path,
                },
                CapabilitySet::rvs_new(),
            ),
            (
                FunctionIdentity {
                    crate_id: 30,
                    def_path: second_impl_path.clone(),
                },
                CapabilitySet::rvs_from_validated("S"),
            ),
        ]);
        let target = FunctionIdentity {
            crate_id: 20,
            def_path: trait_method,
        };
        let voted = rvs_target_trait_vote_caps(&graph, &inferred, &target)
            .expect("never: trait has implementations");
        let implementation_ids = rvs_target_trait_implementation_identities(&graph, &target);
        let output = format!(
            "caps={}\nproduction_fallback={}\n",
            voted.rvs_letters(),
            implementation_ids.contains(&FunctionIdentity {
                crate_id: 30,
                def_path: second_impl_path,
            }),
        );
        rvs_snapshot_BIS(
            "test_20260716_test_trait_vote_falls_back_to_production_implementation",
            &output,
        );

        assert_eq!(voted.rvs_letters(), "S");
        assert!(output.contains("production_fallback=true"));
    }

    #[test]
    fn test_20260716_offline_emissions_reject_empty_anchor_sets() {
        let emissions = vec![OfflineCapsEmission {
            lint: OfflineCapsLint::CallViolation,
            span_anchors: BTreeSet::new(),
            message: "unanchored".to_string(),
        }];
        let serialize_error = rvs_serialize_emissions(&emissions).unwrap_err();
        let parse_error = rvs_parse_emissions(
            r#"[{"lint":"call_violation","span_anchors":[],"message":"unanchored"}]"#,
        )
        .unwrap_err();
        let output = format!("serialize={serialize_error}\nparse={parse_error}\n");
        rvs_snapshot_BIS(
            "test_20260716_offline_emissions_reject_empty_anchor_sets",
            &output,
        );

        assert!(serialize_error.contains("anchor"));
        assert!(parse_error.contains("anchor"));
    }

    #[test]
    fn test_20260716_incomplete_caps_warning_uses_one_anchor_per_caller() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_first"),
            rvs_node(&["dependency::incomplete"]),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_second"),
            rvs_node(&["dependency::incomplete"]),
        );
        let mut info = CapabilityInfo::rvs_migrated_v1(
            CapabilitySet::rvs_new(),
            CapabilityCompleteness::Unknown,
        );
        info.rvs_with_source_M(CapabilitySource {
            layer: "deps".to_string(),
            file: PathBuf::from("caps/deps"),
            line: 2,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from("dependency::incomplete"), info);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::IncompleteCapsKnowledge)
            .expect("never: incomplete knowledge emits a warning");
        let output = format!("anchors={}\n", emission.span_anchors.len());
        rvs_snapshot_BIS(
            "test_20260716_incomplete_caps_warning_uses_one_anchor_per_caller",
            &output,
        );

        assert_eq!(emission.span_anchors.len(), 2);
    }

    #[test]
    fn test_20260715_offline_caps_emissions_round_trip() {
        let emissions = vec![OfflineCapsEmission {
            lint: OfflineCapsLint::CallViolation,
            span_anchors: BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_handle"),
                },
                call_site: None,
                expectation_only: false,
            }]),
            message: "missing S capability".to_string(),
        }];

        let json = rvs_serialize_emissions(&emissions).unwrap();
        let parsed = rvs_parse_emissions(&json).unwrap();
        rvs_snapshot_BIS(
            "test_20260715_offline_caps_emissions_round_trip",
            &(json + "\n"),
        );

        assert_eq!(parsed, emissions);
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
        assert_eq!(diagnostic.call_site_anchors.len(), 2);
    }

    #[test]
    fn test_20260715_unknown_only_suffix_does_not_claim_suffix_is_absent() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_call"),
            rvs_node(&["dep::rvs_fetch_E"]),
        );
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &CapsMap::rvs_new(), &local);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee)
            .expect("never: unknown-only suffix remains an unknown callee");
        rvs_snapshot_BIS(
            "test_20260715_unknown_only_suffix_does_not_claim_suffix_is_absent",
            &format!("{}\n", diagnostic.message),
        );

        assert!(
            diagnostic
                .message
                .contains("no valid capability declaration")
        );
        assert!(!diagnostic.message.contains("has no rvs_ suffix"));
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
        assert_eq!(diagnostic.call_site_anchors.len(), 2);
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
        let caps = rvs_make_capsmap(&[("dep::effect", "S")]);
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
        let caps = rvs_make_capsmap(&[("dep::effect", "S")]);
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
