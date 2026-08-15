use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::artifacts::{CallSiteIdentity, FnGraph, FnNode, FunctionIdentity};
use crate::callgraph::rvs_is_std_like_def_path;
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
use crate::symbols::{CrateName, DefPath, FnName};

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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OfflineCapsCallAnchor {
    caller: FunctionIdentity,
    call_site: CallSiteIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TargetCallUsage {
    caller: FunctionIdentity,
    callee: FunctionIdentity,
    call_site: Option<CallSiteIdentity>,
}

type UnknownCalleeGroups = BTreeMap<String, BTreeSet<TargetCallUsage>>;

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

    pub(crate) fn rvs_emissions(&self, _graph: &FnGraph) -> Vec<OfflineCapsEmission> {
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
                    })
                    .collect();
                if !diagnostic.call_site_anchors.is_empty() {
                    let callers_with_call_sites: BTreeSet<&FunctionIdentity> = diagnostic
                        .call_site_anchors
                        .iter()
                        .map(|anchor| &anchor.caller)
                        .collect();
                    span_anchors.retain(|anchor| {
                        anchor.call_site.is_some()
                            || !callers_with_call_sites.contains(&anchor.identity)
                    });
                    for anchor in &diagnostic.call_site_anchors {
                        span_anchors.insert(OfflineCapsEmissionAnchor {
                            identity: anchor.caller.clone(),
                            call_site: Some(anchor.call_site.clone()),
                        });
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
    let validated = rvs_validate_emissions(emissions)?;
    serde_json::to_string(validated)
        .map_err(|error| format!("cannot serialize offline caps emissions: {error}"))
}

pub(crate) fn rvs_parse_emissions(json: &str) -> Result<Vec<OfflineCapsEmission>, String> {
    let emissions: Vec<OfflineCapsEmission> = serde_json::from_str(json)
        .map_err(|error| format!("cannot parse offline caps emissions: {error}"))?;
    rvs_validate_emissions(&emissions)?;
    Ok(emissions)
}

fn rvs_validate_emissions(
    emissions: &[OfflineCapsEmission],
) -> Result<&[OfflineCapsEmission], String> {
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
    Ok(emissions)
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
    diagnostic_crate_ids: BTreeSet<u64>,
}

#[derive(Debug)]
struct IncompleteCapsUsage {
    layer: String,
    file: String,
    completeness: CapabilityCompleteness,
    bases: BTreeSet<&'static str>,
    usages: BTreeSet<TargetCallUsage>,
    callees: BTreeMap<FunctionIdentity, String>,
}

#[derive(Debug)]
pub(crate) struct TargetTraitImplOutlierGroup {
    pub(crate) outlier: TraitImplOutlier,
    pub(crate) crate_ids: BTreeSet<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct NodeId(usize);

fn rvs_node_slot<T>(values: &[T], target_id: NodeId) -> &T {
    debug_assert!(
        target_id.0 < values.len(),
        "target id belongs to this index"
    );
    values
        .get(target_id.0)
        .expect("never: target id belongs to this analysis index")
}

fn rvs_node_slot_M<T>(values: &mut [T], target_id: NodeId) -> &mut T {
    debug_assert!(
        target_id.0 < values.len(),
        "target id belongs to this index"
    );
    values
        .get_mut(target_id.0)
        .expect("never: target id belongs to this analysis index")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BorrowedFunctionIdentity<'a> {
    crate_id: u64,
    def_path: &'a DefPath,
}

impl<'a> BorrowedFunctionIdentity<'a> {
    fn rvs_from_identity(identity: &'a FunctionIdentity) -> Self {
        Self {
            crate_id: identity.crate_id,
            def_path: &identity.def_path,
        }
    }
}

#[derive(Debug)]
struct IndexedNode<'a> {
    def_path: &'a DefPath,
    node: &'a FnNode,
    classification: FunctionClassification,
    is_local_port: bool,
}

impl IndexedNode<'_> {
    fn rvs_identity(&self) -> FunctionIdentity {
        FunctionIdentity {
            crate_id: self.node.crate_id,
            def_path: self.def_path.clone(),
        }
    }
}

#[derive(Debug)]
struct IndexedCall<'a> {
    callee: &'a FunctionIdentity,
    call_sites: Vec<&'a CallSiteIdentity>,
    local_target: Option<NodeId>,
}

#[derive(Debug)]
struct TargetAnalysisIndex<'a> {
    nodes: Vec<IndexedNode<'a>>,
    identities: HashMap<BorrowedFunctionIdentity<'a>, NodeId>,
    nodes_by_path: HashMap<&'a DefPath, Vec<NodeId>>,
    calls: Vec<Vec<IndexedCall<'a>>>,
    #[allow(dead_code, reason = "retained for diagnostic anchor construction")]
    reverse_calls: Vec<Vec<NodeId>>,
    #[allow(dead_code, reason = "retained for offline outlier test compatibility")]
    trait_implementations: BTreeMap<DefPath, BTreeMap<&'a DefPath, Vec<NodeId>>>,
    vote_inputs: Vec<Vec<Vec<NodeId>>>,
    #[allow(dead_code, reason = "retained for diagnostic anchor construction")]
    reverse_votes: Vec<Vec<NodeId>>,
}

impl<'a> TargetAnalysisIndex<'a> {
    fn rvs_build(graph: &'a FnGraph, local_scope: &LocalScope) -> Self {
        let local_port_operations: BTreeSet<DefPath> = graph
            .rvs_iter()
            .filter(|(def_path, node)| {
                local_scope.rvs_contains_target(def_path, node.crate_provenance)
                    && node.facts.is_port_method
            })
            .map(|(def_path, _)| def_path.clone())
            .collect();
        let mut nodes = Vec::new();
        let mut identities = HashMap::new();
        let mut nodes_by_path: HashMap<&DefPath, Vec<NodeId>> = HashMap::new();
        for (def_path, node) in graph.rvs_iter() {
            let node_id = NodeId(nodes.len());
            let is_local_port = (local_scope.rvs_contains_target(def_path, node.crate_provenance)
                && node.facts.is_port_method)
                || def_path
                    .rvs_trait_method_identity()
                    .is_some_and(|identity| {
                        local_port_operations.contains(&identity.rvs_trait_method_path())
                    });
            identities.insert(
                BorrowedFunctionIdentity {
                    crate_id: node.crate_id,
                    def_path,
                },
                node_id,
            );
            nodes_by_path.entry(def_path).or_default().push(node_id);
            nodes.push(IndexedNode {
                def_path,
                node,
                classification: FunctionClassification::rvs_new(local_scope, def_path, node)
                    .rvs_with_port(is_local_port),
                is_local_port,
            });
        }

        let mut calls = Vec::with_capacity(nodes.len());
        let mut reverse_calls = vec![Vec::new(); nodes.len()];
        for (caller_index, record) in nodes.iter().enumerate() {
            let caller_id = NodeId(caller_index);
            let mut call_sites_by_callee: HashMap<&FunctionIdentity, Vec<&CallSiteIdentity>> =
                HashMap::new();
            for call_site in &record.node.call_sites {
                call_sites_by_callee
                    .entry(&call_site.callee)
                    .or_default()
                    .push(call_site);
            }
            let mut indexed_calls = Vec::with_capacity(record.node.calls.len());
            for callee in record.node.calls.keys() {
                let local_target = identities
                    .get(&BorrowedFunctionIdentity::rvs_from_identity(callee))
                    .copied()
                    .or_else(|| {
                        nodes_by_path
                            .get(&callee.def_path)
                            .and_then(|ids| ids.first().copied())
                    });
                if let Some(callee_id) = local_target {
                    rvs_node_slot_M(&mut reverse_calls, callee_id).push(caller_id);
                }
                indexed_calls.push(IndexedCall {
                    callee,
                    call_sites: call_sites_by_callee.remove(callee).unwrap_or_default(),
                    local_target,
                });
            }
            calls.push(indexed_calls);
        }

        let mut trait_implementations: BTreeMap<DefPath, BTreeMap<&DefPath, Vec<NodeId>>> =
            BTreeMap::new();
        for (node_index, record) in nodes.iter().enumerate() {
            if !record.node.is_trait_impl {
                continue;
            }
            let Some(identity) = record.def_path.rvs_trait_method_identity() else {
                continue;
            };
            trait_implementations
                .entry(identity.rvs_trait_method_path())
                .or_default()
                .entry(record.def_path)
                .or_default()
                .push(NodeId(node_index));
        }

        let mut vote_inputs = vec![Vec::new(); nodes.len()];
        let mut reverse_votes = vec![Vec::new(); nodes.len()];
        for (node_index, record) in nodes.iter().enumerate() {
            let Some(implementations) = trait_implementations.get(record.def_path) else {
                continue;
            };
            let trait_node_id = NodeId(node_index);
            for implementation_nodes in implementations.values() {
                let cohort: Vec<NodeId> = implementation_nodes
                    .iter()
                    .copied()
                    .filter(|implementation_id| {
                        rvs_node_slot(&nodes, *implementation_id).node.has_body
                    })
                    .collect();
                if cohort.is_empty() {
                    continue;
                }
                for implementation_id in &cohort {
                    rvs_node_slot_M(&mut reverse_votes, *implementation_id).push(trait_node_id);
                }
                rvs_node_slot_M(&mut vote_inputs, trait_node_id).push(cohort);
            }
        }

        Self {
            nodes,
            identities,
            nodes_by_path,
            calls,
            reverse_calls,
            trait_implementations,
            vote_inputs,
            reverse_votes,
        }
    }

    fn rvs_target(&self, node_id: NodeId) -> &IndexedNode<'a> {
        rvs_node_slot(&self.nodes, node_id)
    }

    #[cfg(test)]
    fn rvs_find_identity(&self, identity: &FunctionIdentity) -> Option<NodeId> {
        self.identities
            .get(&BorrowedFunctionIdentity::rvs_from_identity(identity))
            .copied()
    }

    fn rvs_find_target(&self, def_path: &DefPath, crate_id: u64) -> Option<NodeId> {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        self.identities
            .get(&BorrowedFunctionIdentity { crate_id, def_path })
            .copied()
    }

    fn rvs_target_ids_for_path(&self, path: &DefPath) -> &[NodeId] {
        self.nodes_by_path.get(path).map_or(&[], Vec::as_slice)
    }

    fn rvs_port_operation_target(&self, node_id: NodeId) -> Option<NodeId> {
        let record = self.rvs_target(node_id);
        if !record.is_local_port || !record.node.is_trait_impl {
            return record.is_local_port.then_some(node_id);
        }
        let operation = record
            .def_path
            .rvs_trait_method_identity()?
            .rvs_trait_method_path();
        let candidates = self.rvs_target_ids_for_path(&operation);
        candidates.iter().copied().find(|candidate_id| {
            self.rvs_target(*candidate_id)
                .def_path
                .rvs_trait_method_identity()
                .is_none()
        })
    }
}

#[derive(Debug, Clone)]
struct NodeInference {
    caps: Vec<CapabilitySet>,
    incomplete: Vec<bool>,
}

impl NodeInference {
    fn rvs_from_prepared(
        index: &TargetAnalysisIndex<'_>,
        prepared: &PreparedLocalAnalysis,
        graph: &FnGraph,
        seed: &CapsMap,
    ) -> Self {
        let resolver = prepared.rvs_resolver(graph, seed);
        let caps = index
            .nodes
            .iter()
            .map(|record| {
                Self::rvs_caps_for_node(record, index, &resolver, prepared.rvs_inferred())
            })
            .collect();
        let incomplete = index
            .nodes
            .iter()
            .map(|record| Self::rvs_is_incomplete_for_node(record, index, prepared))
            .collect();
        NodeInference { caps, incomplete }
    }

    fn rvs_caps_for_node(
        record: &IndexedNode<'_>,
        index: &TargetAnalysisIndex<'_>,
        resolver: &CalleeCapsResolver<'_>,
        inferred: &BTreeMap<DefPath, CapabilitySet>,
    ) -> CapabilitySet {
        if record.is_local_port
            && let Some(&node_id) = index.identities.get(&BorrowedFunctionIdentity {
                crate_id: record.node.crate_id,
                def_path: record.def_path,
            })
            && let Some(contract_id) = index.rvs_port_operation_target(node_id)
        {
            let contract_path = index.rvs_target(contract_id).def_path;
            if let Some(caps) = inferred.get(contract_path) {
                return caps.clone();
            }
        }
        if let Some(caps) = inferred.get(record.def_path) {
            return caps.clone();
        }
        resolver
            .rvs_exact_caps(record.def_path)
            .or_else(|| {
                ParsedFunctionName::rvs_parse(record.def_path.rvs_as_str()).rvs_declared_caps()
            })
            .unwrap_or_default()
    }

    fn rvs_is_incomplete_for_node(
        record: &IndexedNode<'_>,
        index: &TargetAnalysisIndex<'_>,
        prepared: &PreparedLocalAnalysis,
    ) -> bool {
        let incomplete_paths = prepared.rvs_incomplete_paths();
        if record.is_local_port
            && let Some(&node_id) = index.identities.get(&BorrowedFunctionIdentity {
                crate_id: record.node.crate_id,
                def_path: record.def_path,
            })
            && let Some(contract_id) = index.rvs_port_operation_target(node_id)
        {
            let contract_path = index.rvs_target(contract_id).def_path;
            if incomplete_paths.contains(contract_path) {
                return true;
            }
        }
        incomplete_paths.contains(record.def_path)
    }

    fn rvs_caps(&self, node_id: NodeId) -> &CapabilitySet {
        rvs_node_slot(&self.caps, node_id)
    }

    fn rvs_is_incomplete(&self, node_id: NodeId) -> bool {
        *rvs_node_slot(&self.incomplete, node_id)
    }

    #[cfg(test)]
    fn rvs_caps_for_identity<'a>(
        &'a self,
        index: &TargetAnalysisIndex<'_>,
        identity: &FunctionIdentity,
    ) -> Option<&'a CapabilitySet> {
        index
            .rvs_find_identity(identity)
            .map(|target_id| self.rvs_caps(target_id))
    }

    #[cfg(test)]
    fn rvs_incomplete_identities(
        &self,
        index: &TargetAnalysisIndex<'_>,
    ) -> BTreeSet<FunctionIdentity> {
        index
            .nodes
            .iter()
            .enumerate()
            .filter(|(target_index, _)| *rvs_node_slot(&self.incomplete, NodeId(*target_index)))
            .map(|(_, target)| target.rvs_identity())
            .collect()
    }
}

type ContractDiagnosticGroups = BTreeMap<
    (
        FnContractMismatchKind,
        FnName,
        Option<CapabilityKey>,
        CapabilityKey,
    ),
    (FnContractDiff, BTreeSet<u64>),
>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CapabilityKey(u16);

type CallCapabilityMismatchGroups = BTreeMap<
    (DefPath, CapabilityKey, CapabilityKey),
    (
        CapabilitySet,
        CapabilitySet,
        Vec<Capability>,
        BTreeSet<u64>,
        BTreeSet<OfflineCapsCallAnchor>,
    ),
>;
type StaticRefDiagnosticGroups = BTreeMap<
    (CapabilityKey, CapabilityKey, CapabilityKey, bool),
    (CapabilitySet, Vec<Capability>, CapabilitySet, BTreeSet<u64>),
>;

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
                BTreeSet::from([self.node.crate_id]),
            )]),
            call_site_anchors: BTreeSet::new(),
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
    let local_scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let analysis = PreparedLocalAnalysis::rvs_prepare(graph, caps, local_crate_names);
    let resolver = analysis.rvs_resolver(graph, caps);
    let target_index = TargetAnalysisIndex::rvs_build(graph, &local_scope);
    let target_caps = NodeInference::rvs_from_prepared(&target_index, &analysis, graph, caps);
    let mut unknown_callees = UnknownCalleeGroups::new();
    let mut incomplete_caps: BTreeMap<String, IncompleteCapsUsage> = BTreeMap::new();
    for (def_path, node) in graph.rvs_iter() {
        let offline_checked =
            target_index
                .rvs_target_ids_for_path(def_path)
                .iter()
                .any(|target_id| {
                    target_index
                        .rvs_target(*target_id)
                        .classification
                        .rvs_is_offline_checked()
                });
        if !offline_checked {
            continue;
        }
        let parsed_name = ParsedFunctionName::rvs_parse(def_path.rvs_as_str());
        let declared_caps = parsed_name.rvs_declared_caps();
        let context = OfflineFnContext {
            def_path,
            node,
            parsed_name,
            declared_caps,
            diagnostic_crate_ids: BTreeSet::from([node.crate_id]),
        };
        rvs_collect_contract_diagnostics_M(&mut report, &context, &target_index, &target_caps);
        rvs_collect_suffix_diagnostics_M(&mut report, &context);
        rvs_collect_static_ref_diagnostics_M(&mut report, &context, &target_index, &target_caps);
        rvs_collect_call_diagnostics_M(
            &mut report,
            &context,
            &resolver,
            &target_index,
            &target_caps,
            &mut unknown_callees,
            &mut incomplete_caps,
        );
    }
    let prepared_outliers: Vec<TargetTraitImplOutlierGroup> = analysis
        .trait_impl_outliers
        .iter()
        .filter_map(|outlier| {
            let node = graph.rvs_get(outlier.implementation.rvs_as_str())?;
            Some(TargetTraitImplOutlierGroup {
                outlier: outlier.clone(),
                crate_ids: BTreeSet::from([node.crate_id]),
            })
        })
        .collect();
    rvs_append_trait_impl_outliers_M(&mut report, &prepared_outliers);
    rvs_append_unknown_callee_diagnostics_M(&mut report, &unknown_callees);
    rvs_append_incomplete_caps_diagnostics_M(&mut report, &incomplete_caps);
    report.diagnostics.sort();
    report
}

fn rvs_append_trait_impl_outliers_M(
    report: &mut OfflineCapsReport,
    outliers: &[TargetTraitImplOutlierGroup],
) {
    for group in outliers {
        let outlier = &group.outlier;
        let crate_ids = group.crate_ids.clone();
        if crate_ids.is_empty() {
            continue;
        }
        let vote_counts = CapabilityPolicy::rvs_propagated_caps()
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

pub(crate) fn rvs_collect_report_trait_impl_outliers(
    graph: &FnGraph,
    _local_crate_names: &BTreeSet<CrateName>,
    analysis: &PreparedLocalAnalysis,
) -> Vec<TargetTraitImplOutlierGroup> {
    analysis
        .trait_impl_outliers
        .iter()
        .filter_map(|outlier| {
            let node = graph.rvs_get(outlier.implementation.rvs_as_str())?;
            Some(TargetTraitImplOutlierGroup {
                outlier: outlier.clone(),
                crate_ids: BTreeSet::from([node.crate_id]),
            })
        })
        .collect()
}

fn rvs_collect_contract_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    index: &TargetAnalysisIndex<'_>,
    inference: &NodeInference,
) {
    let mut groups = ContractDiagnosticGroups::new();
    for crate_id in context.diagnostic_crate_ids.iter().copied() {
        let target_id = index
            .rvs_find_target(context.def_path, crate_id)
            .expect("never: selected diagnostic target belongs to the target index");
        let contract_target_id = index
            .rvs_port_operation_target(target_id)
            .unwrap_or(target_id);
        let diff = rvs_contract_diff_for_expected_caps(
            context.def_path,
            inference.rvs_caps(contract_target_id).clone(),
            inference.rvs_is_incomplete(contract_target_id),
        );
        for kind in rvs_selected_contract_mismatch_kinds(&diff) {
            let key = (
                kind,
                diff.expected_name.clone(),
                diff.declared_public_caps.as_ref().map(rvs_capability_key),
                rvs_capability_key(&diff.expected_public_caps),
            );
            groups
                .entry(key)
                .or_insert_with(|| (diff.clone(), BTreeSet::new()))
                .1
                .insert(crate_id);
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

pub(crate) fn rvs_uncovered_test_functions(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> BTreeSet<FunctionIdentity> {
    let local_scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let unresolved_test_calls: BTreeSet<&str> = graph
        .rvs_iter()
        .filter(|(_, node)| node.is_test)
        .flat_map(|(_, node)| node.unresolved_test_calls.iter().map(String::as_str))
        .collect();
    let covered: BTreeSet<FunctionIdentity> = graph.rvs_test_reachable_identities();

    let mut candidates = Vec::new();
    for (def_path, node) in graph.rvs_iter() {
        if !node.has_body || !node.is_coverage_candidate {
            continue;
        }
        let identity = FunctionIdentity {
            crate_id: node.crate_id,
            def_path: def_path.clone(),
        };
        if !local_scope.rvs_contains_identity(&identity) {
            continue;
        }
        let parsed = ParsedFunctionName::rvs_parse(def_path.rvs_as_str());
        if !parsed.rvs_has_rvs_prefix() {
            continue;
        }
        let mut caps = parsed.rvs_known_caps().clone();
        if node.facts.is_port_method {
            caps.rvs_insert_M(Capability::P);
        }
        if CapabilityPolicy::rvs_is_ok(&caps) {
            candidates.push(identity);
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
            uncovered.insert(identity);
        }
    }
    uncovered
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
    index: &TargetAnalysisIndex<'_>,
    inference: &NodeInference,
) {
    let mut groups: StaticRefDiagnosticGroups = BTreeMap::new();
    for crate_id in context.diagnostic_crate_ids.iter().copied() {
        let target_id = index
            .rvs_find_target(context.def_path, crate_id)
            .expect("never: selected diagnostic target belongs to the target index");
        let record = index.rvs_target(target_id);
        let is_port_body = record.is_local_port;
        let allowed = if let Some(contract_target_id) = index.rvs_port_operation_target(target_id) {
            inference.rvs_caps(contract_target_id).clone()
        } else {
            let Some(declared) = context.declared_caps.as_ref() else {
                continue;
            };
            declared.clone()
        };
        let facts = record.node.facts;
        let required = CapabilityPolicy::rvs_static_caps(facts);
        let missing: Vec<_> = [Capability::S, Capability::T, Capability::U]
            .into_iter()
            .filter(|capability| {
                required.rvs_contains(*capability) && !allowed.rvs_contains(*capability)
            })
            .collect();
        if !missing.is_empty() {
            let mut missing_caps = CapabilitySet::rvs_new();
            for capability in &missing {
                missing_caps.rvs_insert_M(*capability);
            }
            let key = (
                rvs_capability_key(&required),
                rvs_capability_key(&missing_caps),
                rvs_capability_key(&allowed),
                is_port_body,
            );
            groups
                .entry(key)
                .or_insert_with(|| (required, missing, allowed, BTreeSet::new()))
                .3
                .insert(crate_id);
        }
    }
    for ((_, _, _, is_port_body), (required, missing, allowed, crate_ids)) in groups {
        if crate_ids.is_empty() {
            continue;
        }
        let mut diagnostic = context.rvs_diagnostic(
            OfflineCapsSeverity::Error,
            OfflineCapsKind::StaticRefRequiresCaps,
            "function touches static/thread-local state without declaring required caps"
                .to_string(),
            vec![
                format!(
                    "{}: {}",
                    if is_port_body {
                        "World Port operation contract"
                    } else {
                        "declared caps"
                    },
                    rvs_format_caps(&allowed)
                ),
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

fn rvs_collect_call_diagnostics_M(
    report: &mut OfflineCapsReport,
    context: &OfflineFnContext<'_>,
    resolver: &CalleeCapsResolver<'_>,
    index: &TargetAnalysisIndex<'_>,
    inference: &NodeInference,
    unknown_callees: &mut UnknownCalleeGroups,
    incomplete_caps: &mut BTreeMap<String, IncompleteCapsUsage>,
) {
    let mut missing_groups = CallCapabilityMismatchGroups::new();
    for crate_id in context.diagnostic_crate_ids.iter().copied() {
        let target_id = index
            .rvs_find_target(context.def_path, crate_id)
            .expect("never: selected diagnostic target belongs to the target index");
        let record = index.rvs_target(target_id);
        if !record.node.has_body {
            continue;
        }
        let port_contract_target = index.rvs_port_operation_target(target_id);
        let caller_caps = if let Some(contract_target_id) = port_contract_target {
            inference.rvs_caps(contract_target_id).clone()
        } else {
            context
                .declared_caps
                .clone()
                .unwrap_or_else(|| inference.rvs_caps(target_id).clone())
        };
        let caller = record.rvs_identity();
        for call in rvs_node_slot(&index.calls, target_id) {
            if rvs_is_test_harness_callee(&call.callee.def_path) {
                continue;
            }
            let usages = rvs_target_call_usages(&caller, call);
            let callee_target = call
                .local_target
                .map(|callee_id| index.rvs_target(callee_id));
            let exact_incomplete = (!callee_target.is_some_and(|target| target.is_local_port))
                .then(|| resolver.rvs_exact_caps_info(&call.callee.def_path))
                .flatten()
                .filter(|info| info.rvs_completeness() != CapabilityCompleteness::Complete);
            if let Some(info) = exact_incomplete {
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
                        bases: BTreeSet::new(),
                        usages: BTreeSet::new(),
                        callees: BTreeMap::new(),
                    });
                usage.bases.insert(info.rvs_basis().rvs_name());
                usage.usages.extend(usages.iter().cloned());
                usage.callees.insert(
                    call.callee.clone(),
                    format!(
                        "known caps: {}, basis={}",
                        rvs_format_caps(info.rvs_caps()),
                        info.rvs_basis().rvs_name()
                    ),
                );
            } else if call.local_target.is_some_and(|callee_id| {
                !index.rvs_target(callee_id).is_local_port && inference.rvs_is_incomplete(callee_id)
            }) {
                let usage = incomplete_caps
                    .entry("<inference>\0<callgraph>\0incomplete".to_string())
                    .or_insert_with(|| IncompleteCapsUsage {
                        layer: "<inference>".to_string(),
                        file: "<callgraph>".to_string(),
                        completeness: CapabilityCompleteness::Incomplete,
                        bases: BTreeSet::from(["inferred"]),
                        usages: BTreeSet::new(),
                        callees: BTreeMap::new(),
                    });
                usage.usages.extend(usages.iter().cloned());
                usage.callees.insert(
                    call.callee.clone(),
                    format!(
                        "known caps: {}, basis=inferred",
                        call.local_target.map_or_else(
                            || "unknown".to_string(),
                            |callee_id| rvs_format_caps(inference.rvs_caps(callee_id)),
                        )
                    ),
                );
            }

            let callee_caps = rvs_target_contract_caps(call, index, inference, resolver);
            let Some(mismatch) = rvs_collect_call_contract_mismatch(
                call.callee.def_path.rvs_as_str(),
                &caller_caps,
                callee_caps.as_ref(),
            ) else {
                continue;
            };
            match mismatch.kind {
                CallContractMismatchKind::UnknownCallee => {
                    unknown_callees
                        .entry(call.callee.def_path.to_string())
                        .or_default()
                        .extend(usages);
                }
                CallContractMismatchKind::MissingCapabilities => {
                    if port_contract_target
                        .is_some_and(|contract_id| inference.rvs_is_incomplete(contract_id))
                    {
                        continue;
                    }
                    let callee_caps = mismatch
                        .callee_caps
                        .expect("never: missing-capability mismatch carries callee caps");
                    let missing: Vec<_> = mismatch.missing_caps.iter().copied().collect();
                    let key = (
                        call.callee.def_path.clone(),
                        rvs_capability_key(&caller_caps),
                        rvs_capability_key(&callee_caps),
                    );
                    let group = missing_groups.entry(key).or_insert_with(|| {
                        (
                            caller_caps.clone(),
                            callee_caps,
                            missing,
                            BTreeSet::new(),
                            BTreeSet::new(),
                        )
                    });
                    group.3.insert(crate_id);
                    for usage in usages {
                        if let Some(call_site) = usage.call_site {
                            group.4.insert(OfflineCapsCallAnchor {
                                caller: usage.caller,
                                call_site,
                            });
                        }
                    }
                }
            }
        }
    }
    for ((callee, _, _), (caller_caps, callee_caps, missing, crate_ids, call_site_anchors)) in
        missing_groups
    {
        if crate_ids.is_empty() {
            continue;
        }
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Error,
            kind: OfflineCapsKind::CallViolation,
            function: context.def_path.clone(),
            span_anchors: if call_site_anchors.is_empty() {
                BTreeMap::from([(context.def_path.clone(), crate_ids)])
            } else {
                BTreeMap::new()
            },
            call_site_anchors,
            message: "caller lacks propagated capabilities required by callee".to_string(),
            details: vec![
                format!("callee: {callee}"),
                format!("caller declared caps: {}", rvs_format_caps(&caller_caps)),
                format!("callee caps: {}", rvs_format_caps(&callee_caps)),
                format!("missing propagated caps: {}", rvs_format_cap_list(&missing)),
            ],
        });
    }
}

fn rvs_target_contract_caps(
    call: &IndexedCall<'_>,
    index: &TargetAnalysisIndex<'_>,
    inference: &NodeInference,
    resolver: &CalleeCapsResolver<'_>,
) -> Option<CapabilitySet> {
    if let Some(callee_id) = call.local_target {
        let target = index.rvs_target(callee_id);
        if target.is_local_port {
            let contract_target_id = index
                .rvs_port_operation_target(callee_id)
                .unwrap_or(callee_id);
            return Some(inference.rvs_caps(contract_target_id).clone());
        }
        return resolver
            .rvs_exact_caps(&call.callee.def_path)
            .or_else(|| {
                ParsedFunctionName::rvs_parse(call.callee.def_path.rvs_as_str()).rvs_declared_caps()
            })
            .or_else(|| {
                (target.node.has_body || !rvs_node_slot(&index.vote_inputs, callee_id).is_empty())
                    .then(|| inference.rvs_caps(callee_id).clone())
            });
    }
    resolver.rvs_exact_caps(&call.callee.def_path).or_else(|| {
        ParsedFunctionName::rvs_parse(call.callee.def_path.rvs_as_str()).rvs_declared_caps()
    })
}

fn rvs_target_call_usages(
    caller: &FunctionIdentity,
    call: &IndexedCall<'_>,
) -> Vec<TargetCallUsage> {
    if call.call_sites.is_empty() {
        return vec![TargetCallUsage {
            caller: caller.clone(),
            callee: call.callee.clone(),
            call_site: None,
        }];
    }
    call.call_sites
        .iter()
        .map(|call_site| TargetCallUsage {
            caller: caller.clone(),
            callee: call.callee.clone(),
            call_site: Some((*call_site).clone()),
        })
        .collect()
}

fn rvs_capability_key(caps: &CapabilitySet) -> CapabilityKey {
    let mut bits = 0u16;
    for capability in caps.rvs_iter() {
        bits |= match capability {
            Capability::A => 1 << 0,
            Capability::B => 1 << 1,
            Capability::I => 1 << 2,
            Capability::M => 1 << 3,
            Capability::P => 1 << 4,
            Capability::S => 1 << 5,
            Capability::T => 1 << 6,
            Capability::U => 1 << 7,
        };
    }
    CapabilityKey(bits)
}

fn rvs_append_incomplete_caps_diagnostics_M(
    report: &mut OfflineCapsReport,
    incomplete_caps: &BTreeMap<String, IncompleteCapsUsage>,
) {
    for usage in incomplete_caps.values() {
        if usage.usages.is_empty() {
            continue;
        }
        let callers: BTreeSet<&FunctionIdentity> =
            usage.usages.iter().map(|call| &call.caller).collect();
        let mut details = vec![
            format!("layer: {}", usage.layer),
            format!("file: {}", usage.file),
            format!("completeness: {}", usage.completeness.rvs_name()),
            format!(
                "knowledge bases: {}",
                usage.bases.iter().copied().collect::<Vec<_>>().join(", ")
            ),
            format!("affected callees: {}", usage.callees.len()),
        ];
        details.extend(usage.callees.iter().take(5).map(|(callee, knowledge)| {
            format!(
                "callee: {} [crate_id={}] ({knowledge})",
                callee.def_path, callee.crate_id
            )
        }));
        if usage.callees.len() > 5 {
            details.push(format!(
                "... and {} more incomplete callees",
                usage.callees.len() - 5
            ));
        }
        details.push(format!("affected callers: {}", callers.len()));
        details.extend(
            callers.iter().take(5).map(|caller| {
                format!("caller: {} [crate_id={}]", caller.def_path, caller.crate_id)
            }),
        );
        if callers.len() > 5 {
            details.push(format!(
                "... and {} more affected callers",
                callers.len() - 5
            ));
        }
        let Some(representative_callee) = usage.callees.keys().next().cloned() else {
            continue;
        };
        details.push(rvs_incomplete_caps_remediation(
            usage,
            &representative_callee,
        ));
        let mut span_anchors: BTreeMap<DefPath, BTreeSet<u64>> = BTreeMap::new();
        for caller in &callers {
            span_anchors
                .entry(caller.def_path.clone())
                .or_default()
                .insert(caller.crate_id);
        }
        let function = usage
            .usages
            .iter()
            .next()
            .map(|usage| usage.caller.def_path.clone())
            .unwrap_or_else(|| DefPath::from(""));
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::IncompleteCapsKnowledge,
            function,
            span_anchors,
            call_site_anchors: BTreeSet::new(),
            message:
                "calls rely on incomplete caps knowledge; checks use known capability lower bounds"
                    .to_string(),
            details,
        });
    }
}

fn rvs_incomplete_caps_remediation(
    usage: &IncompleteCapsUsage,
    representative_callee: &FunctionIdentity,
) -> String {
    let path = &representative_callee.def_path;
    let identity_context = format!(
        "callee target crate_id={}: {}",
        representative_callee.crate_id, representative_callee.def_path
    );
    if usage.layer == "<inference>" {
        return format!(
            "local inference is incomplete because the call graph reaches unknown or incomplete knowledge ({identity_context}); inspect `cargo rivus why '{path}' .`; `<inference>` is computed during check and is not a refreshable caps layer"
        );
    }

    let has_inferred = usage.bases.contains("inferred") || usage.bases.contains("trait_vote");
    if usage.layer == "std" {
        if has_inferred {
            return format!(
                "inferred standard-library knowledge remains incomplete because inference reached an opaque body or incomplete trait contribution ({identity_context}); inspect `cargo rivus why '{path}' .`; rerunning `cargo rivus infer-std -o caps/std` without resolving that boundary will preserve the lower bound"
            );
        }
        return format!(
            "inspect `cargo rivus why '{path}' .` for {identity_context}, then run `cargo rivus infer-std -o caps/std` to replace migrated standard-library knowledge; if inferred records remain incomplete, inspect their opaque bodies or incomplete trait contributions rather than marking them complete without evidence"
        );
    }

    if usage.layer == "deps" {
        if has_inferred {
            return format!(
                "inferred dependency knowledge for '{path}' remains incomplete because a dependency body is opaque or reaches other incomplete knowledge ({identity_context}); inspect `cargo rivus why '{path}' .`; rerunning `cargo rivus infer-capsmap -o caps/deps` without resolving that boundary will preserve the lower bound"
            );
        }
        return format!(
            "inspect `cargo rivus why '{path}' .` for {identity_context}, then run `cargo rivus infer-capsmap -o caps/deps` to replace migrated dependency knowledge; if generated records remain incomplete, inspect their opaque or incomplete prerequisites rather than marking them complete without evidence"
        );
    }

    format!(
        "knowledge in layer '{}' remains incomplete for {identity_context}; inspect `cargo rivus why '{path}' .` and its opaque or incomplete prerequisites before adding an explicit correction",
        usage.layer
    )
}

fn rvs_append_unknown_callee_diagnostics_M(
    report: &mut OfflineCapsReport,
    unknown_callees: &UnknownCalleeGroups,
) {
    for (callee, usages) in unknown_callees {
        let readable_callers: BTreeSet<String> = usages
            .iter()
            .map(|usage| usage.caller.def_path.to_string())
            .collect();
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
        let all_have_call_sites = usages.iter().all(|usage| usage.call_site.is_some());
        let call_site_anchors = usages
            .iter()
            .filter_map(|usage| {
                usage
                    .call_site
                    .as_ref()
                    .cloned()
                    .map(|call_site| OfflineCapsCallAnchor {
                        caller: usage.caller.clone(),
                        call_site,
                    })
            })
            .collect();
        let mut span_anchors: BTreeMap<DefPath, BTreeSet<u64>> = BTreeMap::new();
        for usage in usages {
            span_anchors
                .entry(usage.caller.def_path.clone())
                .or_default()
                .insert(usage.caller.crate_id);
        }
        report.diagnostics.push(OfflineCapsDiagnostic {
            severity: OfflineCapsSeverity::Warning,
            kind: OfflineCapsKind::UnknownCallee,
            function: callee_path,
            span_anchors: if all_have_call_sites {
                BTreeMap::new()
            } else {
                span_anchors
            },
            call_site_anchors: if all_have_call_sites {
                call_site_anchors
            } else {
                BTreeSet::new()
            },
            message: format!("callee '{callee}' {missing_declaration} and no caps/ entry"),
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
    use crate::artifacts::{CallEdgeType, CallSiteSource, CrateProvenance, FnNode, FnSource};
    use crate::capability::{CapabilityBasis, CapabilityFacts, CapabilityInfo, CapabilitySource};
    use crate::symbols::CapsMapKey;
    use crate::test_support::{rvs_make_capsmap, rvs_snapshot_BIS};
    use std::path::PathBuf;

    fn rvs_node(calls: &[&str]) -> FnNode {
        let mut node = FnNode::default();
        node.crate_id = 1;
        node.crate_provenance = CrateProvenance::PrimaryPackage;
        node.is_production = true;
        node.is_coverage_candidate = true;
        node.calls = calls
            .iter()
            .map(|call| {
                (
                    FunctionIdentity {
                        crate_id: 2,
                        def_path: DefPath::from(*call),
                    },
                    CallEdgeType::Strong,
                )
            })
            .collect();
        node.sources
            .insert(FnSource::rvs_new(PathBuf::from("src/lib.rs"), 1, 2));
        node
    }

    fn rvs_set_target_crates_M(node: &mut FnNode, crate_ids: &[u64]) {
        if let Some(&crate_id) = crate_ids.first() {
            node.crate_id = crate_id;
        }
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
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dep::environment"]);
        outlier.is_trait_impl = true;
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
        let mut info = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
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
        assert!(output.contains("cargo rivus infer-capsmap -o caps/deps"));
        assert!(!output.contains("add reviewed corrections to caps/ext"));
    }

    #[test]
    fn test_20260721_fresh_inferred_std_incomplete_warning_is_actionable() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_handle"),
            rvs_node(&["alloc::boxed::Box::new_uninit"]),
        );
        let mut info = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Incomplete,
        );
        info.rvs_with_source_M(CapabilitySource {
            layer: "std".to_string(),
            file: PathBuf::from("caps/std"),
            line: 146,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from("alloc::boxed::Box::new_uninit"), info);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260721_fresh_inferred_std_incomplete_warning_is_actionable",
            &output,
        );

        assert!(output.contains("inferred standard-library knowledge remains incomplete"));
        assert!(output.contains("cargo rivus why 'alloc::boxed::Box::new_uninit' ."));
        assert!(!output.contains("replace migrated standard-library knowledge"));
        assert!(!output.contains("add reviewed corrections to caps/ext"));
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
        let mut caller = rvs_node(&[]);
        caller.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::Parser::rvs_parse"),
            },
            CallEdgeType::Strong,
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_use_parser"), caller);

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
        assert!(output.contains("`<inference>` is computed during check"));
        assert!(output.contains("cargo rivus why 'demo::Parser::rvs_parse' ."));
        assert!(!output.contains("refresh generated layer '<inference>'"));
        assert!(!output.contains("add reviewed corrections to caps/ext"));
    }

    #[test]
    fn test_20260715_call_emission_is_scoped_to_violating_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_node(&[]);
        caller.crate_id = 10;
        let callee_identity = FunctionIdentity {
            crate_id: 50,
            def_path: DefPath::from("dependency::effect"),
        };
        caller.calls = BTreeMap::from([(callee_identity.clone(), CallEdgeType::Strong)]);
        caller.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: callee_identity,
            occurrence: 0,
            source: None,
        }]);
        caller.is_production = true;
        caller.is_coverage_candidate = true;
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
                    format!("{}:{}", anchor.identity.crate_id, anchor.identity.def_path)
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
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 10,
                    def_path: DefPath::from("demo::rvs_call"),
                },
                call_site: Some(CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 50,
                        def_path: DefPath::from("dependency::effect"),
                    },
                    occurrence: 0,
                    source: None,
                }),
            }])
        );
    }

    #[test]
    fn test_20260715_non_call_emissions_are_scoped_to_violating_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut node = rvs_node(&[]);
        node.crate_id = 10;
        node.facts = static_facts;
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
                .expect("never: flat model behavior produces an emission");
            output.push_str(&format!("{lint:?}={:?}\n", emission.span_anchors));
            assert_eq!(
                emission.span_anchors,
                BTreeSet::from([OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("demo::rvs_read_cache"),
                    },
                    call_site: None,
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
        callee.crate_id = 10;
        callee.facts = static_facts;
        let callee_path = DefPath::from("demo::effect");
        graph.rvs_insert_M(callee_path.clone(), callee);

        let callee_identity = FunctionIdentity {
            crate_id: 10,
            def_path: callee_path,
        };
        let mut caller = rvs_node(&[]);
        caller.crate_id = 10;
        caller.calls = BTreeMap::from([(callee_identity, CallEdgeType::Strong)]);
        caller.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: FunctionIdentity {
                crate_id: 10,
                def_path: DefPath::from("demo::effect"),
            },
            occurrence: 0,
            source: None,
        }]);
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
            .expect("never: static-using callee violates the pure caller contract");
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
                call_site: Some(CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("demo::effect"),
                    },
                    occurrence: 0,
                    source: None,
                }),
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
        node.facts = facts;
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
            }])
        );
    }

    #[test]
    fn test_20260715_bodyless_trait_contract_keeps_vote_derived_anchor() {
        let mut graph = FnGraph::rvs_new();
        let declaration_path = DefPath::from("demo::Parser::parse");
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        declaration.has_body = false;
        graph.rvs_insert_M(declaration_path.clone(), declaration);
        for implementation in ["demo::First", "demo::Second"] {
            let mut node = rvs_node(&["dependency::effect"]);
            node.is_trait_impl = true;
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
            }])
        );
    }

    #[test]
    fn test_20260715_bodyless_trait_vote_is_scoped_to_target_identity() {
        let mut graph = FnGraph::rvs_new();
        let declaration_path = DefPath::from("demo::Parser::rvs_parse");
        let mut declaration = rvs_node(&[]);
        declaration.crate_id = 10;
        declaration.has_body = false;
        graph.rvs_insert_M(declaration_path.clone(), declaration);
        for implementation in ["demo::First", "demo::Second"] {
            let mut node = rvs_node(&["dependency::effect"]);
            node.is_trait_impl = true;
            node.crate_id = 10;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                node,
            );
        }
        let mut caller = rvs_node(&[]);
        caller.crate_id = 10;
        caller.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 10,
                def_path: declaration_path.clone(),
            },
            CallEdgeType::Strong,
        )]);
        caller.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: FunctionIdentity {
                crate_id: 10,
                def_path: declaration_path.clone(),
            },
            occurrence: 0,
            source: None,
        }]);
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
            .expect("never: effectful trait-vote target requires an S marker");
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
            }])
        );
    }

    #[test]
    fn test_20260729_required_target_trait_vote_preserves_signature_caps() {
        let mut graph = FnGraph::rvs_new();
        let declaration_path = DefPath::from("demo::Transformer::rvs_transform_AMU");
        let signature_facts = CapabilityFacts {
            has_async: true,
            has_mut_param: true,
            is_unsafe_fn: true,
            ..CapabilityFacts::default()
        };
        let mut declaration = rvs_node(&[]);
        declaration.facts = signature_facts;
        declaration.has_body = false;
        let target = declaration.rvs_test_target_M(1);
        target.facts = signature_facts;
        target.has_body = false;
        graph.rvs_insert_M(declaration_path.clone(), declaration);

        let mut implementation = rvs_node(&["dependency::effect"]);
        implementation.is_trait_impl = true;
        implementation.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryTransformer::rvs_transform_AMU@demo::Transformer"),
            implementation,
        );

        let caps = rvs_make_capsmap(&[("dependency::effect", "S")]);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &scoped_graph);
        let index = TargetAnalysisIndex::rvs_build(&scoped_graph, &local_scope);
        let target_inference =
            NodeInference::rvs_from_prepared(&index, &analysis, &scoped_graph, &caps);
        let identity = FunctionIdentity {
            crate_id: 1,
            def_path: declaration_path,
        };
        let inferred = target_inference
            .rvs_caps_for_identity(&index, &identity)
            .expect("never: target inference covers the required trait method");
        let output = format!("required_caps={}\n", inferred.rvs_letters());
        rvs_snapshot_BIS(
            "test_20260729_required_target_trait_vote_preserves_signature_caps",
            &output,
        );

        assert_eq!(inferred.rvs_letters(), "AMSU");
    }

    #[test]
    fn test_20260729_provided_target_trait_vote_preserves_barriers() {
        let mut graph = FnGraph::rvs_new();
        let provided_path = DefPath::from("demo::Loader::rvs_load_BS");
        graph.rvs_insert_M(provided_path.clone(), rvs_node(&["dependency::blocking"]));
        let mut provided_implementation = rvs_node(&["dependency::effect"]);
        provided_implementation.is_trait_impl = true;
        provided_implementation.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryLoader::rvs_load_BS@demo::Loader"),
            provided_implementation,
        );

        let exact_path = DefPath::from("demo::ExactLoader::rvs_load_I");
        graph.rvs_insert_M(exact_path.clone(), rvs_node(&["dependency::unknown"]));
        let mut exact_implementation = rvs_node(&["dependency::unknown"]);
        exact_implementation.is_trait_impl = true;
        exact_implementation.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::DiskLoader::rvs_load_I@demo::ExactLoader"),
            exact_implementation,
        );

        let port_path = DefPath::from("demo::LoaderClient::rvs_load_P");
        let mut port = rvs_node(&["dependency::unknown"]);
        port.facts.is_port_method = true;
        port.facts.is_port_method = true;
        graph.rvs_insert_M(port_path.clone(), port);
        let mut port_implementation = rvs_node(&["dependency::unknown"]);
        port_implementation.is_trait_impl = true;
        port_implementation.facts.is_port_method = true;
        let target = port_implementation.rvs_test_target_M(1);
        target.is_trait_impl = true;
        target.facts.is_port_method = true;
        graph.rvs_insert_M(
            DefPath::from("demo::DiskLoaderClient::rvs_load_P@demo::LoaderClient"),
            port_implementation,
        );

        let mut caps = rvs_make_capsmap(&[
            ("dependency::blocking", "B"),
            (exact_path.rvs_as_str(), "I"),
        ]);
        caps.rvs_insert_info_M(
            CapsMapKey::from("dependency::effect"),
            CapabilityInfo::rvs_new(
                CapabilitySet::rvs_from_validated("S"),
                CapabilityBasis::Inferred,
                CapabilityCompleteness::Incomplete,
            ),
        );
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &scoped_graph);
        let index = TargetAnalysisIndex::rvs_build(&scoped_graph, &local_scope);
        let target_inference =
            NodeInference::rvs_from_prepared(&index, &analysis, &scoped_graph, &caps);
        let provided_identity = FunctionIdentity {
            crate_id: 1,
            def_path: provided_path,
        };
        let exact_identity = FunctionIdentity {
            crate_id: 1,
            def_path: exact_path,
        };
        let port_identity = FunctionIdentity {
            crate_id: 1,
            def_path: port_path,
        };
        let provided_caps = target_inference
            .rvs_caps_for_identity(&index, &provided_identity)
            .expect("never: target inference covers the provided trait method")
            .rvs_letters();
        let exact_caps = target_inference
            .rvs_caps_for_identity(&index, &exact_identity)
            .expect("never: target inference covers the exact trait method")
            .rvs_letters();
        let port_caps = target_inference
            .rvs_caps_for_identity(&index, &port_identity)
            .expect("never: target inference covers the Port trait method")
            .rvs_letters();
        let output = format!(
            "provided_caps={provided_caps}\nprovided_incomplete={}\nexact_caps={exact_caps}\nexact_incomplete={}\nport_caps={port_caps}\nport_incomplete={}\n",
            index
                .rvs_find_identity(&provided_identity)
                .is_some_and(|target_id| target_inference.rvs_is_incomplete(target_id)),
            index
                .rvs_find_identity(&exact_identity)
                .is_some_and(|target_id| target_inference.rvs_is_incomplete(target_id)),
            index
                .rvs_find_identity(&port_identity)
                .is_some_and(|target_id| target_inference.rvs_is_incomplete(target_id)),
        );
        rvs_snapshot_BIS(
            "test_20260729_provided_target_trait_vote_preserves_barriers",
            &output,
        );

        assert_eq!(provided_caps, "BS");
        assert!(
            index
                .rvs_find_identity(&provided_identity)
                .is_some_and(|target_id| target_inference.rvs_is_incomplete(target_id))
        );
        assert_eq!(exact_caps, "I");
        assert!(
            index
                .rvs_find_identity(&exact_identity)
                .is_some_and(|target_id| !target_inference.rvs_is_incomplete(target_id))
        );
        assert_eq!(port_caps, "P");
        assert!(
            index
                .rvs_find_identity(&port_identity)
                .is_some_and(|target_id| target_inference.rvs_is_incomplete(target_id))
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
        let production = node.rvs_test_target_M(10);
        production.sources = BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 1, 2)]);
        production.is_production = true;
        production.is_coverage_candidate = true;
        let test = node.rvs_test_target_M(20);
        test.facts = static_facts;
        test.sources = BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 3, 4)]);
        test.is_test_compilation = true;
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
            }])
        );
    }

    #[test]
    fn test_20260729_offline_diagnostic_roles_are_target_scoped() {
        let path = DefPath::from("demo::rvs_shared");
        let production_source = FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 17);
        let mut node = FnNode {
            sources: BTreeSet::from([production_source.clone()]),
            ..rvs_node(&[])
        };
        node.crate_id = 10;
        node.facts.has_static_ref = true;
        node.is_production = true;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(path.clone(), node);

        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let emissions = report.rvs_emissions(&graph);
        let static_emissions = emissions
            .iter()
            .filter(|emission| emission.lint == OfflineCapsLint::StaticRef)
            .collect::<Vec<_>>();
        let anchors = static_emissions
            .iter()
            .flat_map(|emission| emission.span_anchors.iter().cloned())
            .collect::<BTreeSet<_>>();
        let output = format!(
            "static_emissions={}\nanchors={anchors:?}\n",
            static_emissions.len(),
        );
        rvs_snapshot_BIS(
            "test_20260729_offline_diagnostic_roles_are_target_scoped",
            &output,
        );

        assert_eq!(static_emissions.len(), 1);
        assert_eq!(
            anchors,
            BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 10,
                    def_path: path,
                },
                call_site: None,
            }])
        );
    }

    #[test]
    fn test_20260715_same_source_test_only_behavior_keeps_diagnostic_anchors() {
        let path = DefPath::from("demo::rvs_variant");
        let production = rvs_node(&[]);
        let mut production_graph = FnGraph::rvs_new();
        production_graph.rvs_insert_M(path.clone(), production);

        let static_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut test = rvs_node(&["dependency::effect"]);
        test.is_test_compilation = true;
        test.facts = static_facts;
        let dependency_call = FunctionIdentity {
            crate_id: 50,
            def_path: DefPath::from("dependency::effect"),
        };
        test.calls = BTreeMap::from([(dependency_call.clone(), CallEdgeType::Strong)]);
        test.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: dependency_call.clone(),
            occurrence: 0,
            source: None,
        }]);
        test.crate_id = 20;
        let mut test_graph = FnGraph::rvs_new();
        test_graph.rvs_insert_M(path.clone(), test);

        let graph = FnGraph::rvs_merge_artifacts(
            vec![production_graph, test_graph],
            &BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let merged_crate_id = graph
            .rvs_get(path.rvs_as_str())
            .map(|node| node.crate_id)
            .unwrap_or(1);
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
            let call_site = (lint == OfflineCapsLint::CallViolation).then(|| CallSiteIdentity {
                callee: dependency_call.clone(),
                occurrence: 0,
                source: None,
            });
            assert_eq!(
                emission.span_anchors,
                BTreeSet::from([OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: merged_crate_id,
                        def_path: path.clone(),
                    },
                    call_site,
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
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dependency::effect"]);
        outlier.is_trait_impl = true;
        outlier.crate_id = 10;
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
            }])
        );
    }

    #[test]
    fn test_20260715_trait_outlier_split_across_targets_keeps_both_anchors() {
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
        let mut outlier_s = rvs_node(&["dependency::side_effect"]);
        outlier_s.is_trait_impl = true;
        outlier_s.crate_id = 10;
        let outlier_s_path = DefPath::from("demo::EnvValueS::rvs_parse@demo::FromString");
        graph.rvs_insert_M(outlier_s_path, outlier_s);
        let mut outlier_t = rvs_node(&["dependency::thread_local"]);
        outlier_t.is_trait_impl = true;
        outlier_t.crate_id = 20;
        let outlier_t_path = DefPath::from("demo::EnvValueT::rvs_parse@demo::FromString");
        graph.rvs_insert_M(outlier_t_path, outlier_t);

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
        assert_eq!(
            anchors
                .iter()
                .map(|a| a.identity.crate_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([10, 20])
        );
    }

    #[test]
    fn test_20260716_target_incompleteness_is_scoped_to_calling_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_node(&[]);
        node.crate_id = 10;
        node.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 50,
                def_path: DefPath::from("dependency::unknown"),
            },
            CallEdgeType::Strong,
        )]);
        node.call_sites.clear();
        let path = DefPath::from("demo::rvs_handle_S");
        graph.rvs_insert_M(path.clone(), node);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph.clone();
        let analysis =
            PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &CapsMap::rvs_new(), &local);
        let empty_caps = CapsMap::rvs_new();
        let local_scope = LocalScope::rvs_for_graph(&local, &scoped_graph);
        let index = TargetAnalysisIndex::rvs_build(&scoped_graph, &local_scope);
        let target_inference =
            NodeInference::rvs_from_prepared(&index, &analysis, &scoped_graph, &empty_caps);
        let incomplete = target_inference.rvs_incomplete_identities(&index);
        let incomplete_output = incomplete.iter().cloned().collect::<Vec<_>>();
        let output = format!("incomplete={incomplete_output:?}\n");
        rvs_snapshot_BIS(
            "test_20260716_target_incompleteness_is_scoped_to_calling_identity",
            &output,
        );

        assert_eq!(
            incomplete,
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
        graph.rvs_insert_M(DefPath::from("demo::Parser::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                node,
            );
        }
        let mut outlier = rvs_node(&["dependency::effect"]);
        outlier.is_trait_impl = true;
        outlier.crate_id = 13;
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
        let mut node_a = rvs_node(&[]);
        node_a.crate_id = 10;
        node_a.facts = static_facts;
        graph.rvs_insert_M(DefPath::from("demo::rvs_read_a"), node_a);
        let mut node_b = rvs_node(&[]);
        node_b.crate_id = 20;
        node_b.facts = thread_local_facts;
        graph.rvs_insert_M(DefPath::from("demo::rvs_read_b"), node_b);

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
        let callee_identity = FunctionIdentity {
            crate_id: 2,
            def_path: DefPath::from("dependency::effect"),
        };
        let mut node = rvs_node(&[]);
        node.calls = BTreeMap::from([(callee_identity.clone(), CallEdgeType::Strong)]);
        node.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: callee_identity,
            occurrence: 0,
            source: None,
        }]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_call"), node);
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
    fn test_20260716_call_site_diagnostic_matches_full_callee_identity() {
        let callee_path = DefPath::from("dependency::effect");
        let effectful_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut callee = rvs_node(&[]);
        callee.facts = effectful_facts;
        callee.crate_id = 50;
        callee.crate_provenance = CrateProvenance::Dependency;

        let mut caller = rvs_node(&[]);
        let callee_identity_50 = FunctionIdentity {
            crate_id: 50,
            def_path: callee_path.clone(),
        };
        caller.calls = BTreeMap::from([(callee_identity_50.clone(), CallEdgeType::Strong)]);
        caller.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: callee_identity_50.clone(),
            occurrence: 0,
            source: None,
        }]);

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(callee_path, callee);
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
            .expect("never: the effectful callee identity violates the pure caller contract");
        let callee_ids = emission
            .span_anchors
            .iter()
            .filter_map(|anchor| anchor.call_site.as_ref())
            .map(|call_site| call_site.callee.crate_id)
            .collect::<Vec<_>>();
        let output = format!("callee_ids={callee_ids:?}\n");
        rvs_snapshot_BIS(
            "test_20260716_call_site_diagnostic_matches_full_callee_identity",
            &output,
        );

        assert_eq!(callee_ids, vec![50]);
    }

    #[test]
    fn test_20260716_test_trait_vote_falls_back_to_production_implementation() {
        let trait_method = DefPath::from("demo::Parser::rvs_parse");
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        graph.rvs_insert_M(trait_method.clone(), declaration);

        let first_impl_path = DefPath::from("demo::Alpha::rvs_parse@demo::Parser");
        let mut first_impl = rvs_node(&[]);
        first_impl.is_trait_impl = true;
        graph.rvs_insert_M(first_impl_path.clone(), first_impl);

        let second_impl_path = DefPath::from("demo::Beta::rvs_parse@demo::Parser");
        let mut second_impl = rvs_node(&["dependency::effect"]);
        second_impl.is_trait_impl = true;
        graph.rvs_insert_M(second_impl_path.clone(), second_impl);

        let target = FunctionIdentity {
            crate_id: 1,
            def_path: trait_method,
        };
        let caps = rvs_make_capsmap(&[("dependency::effect", "S")]);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut scoped_graph = graph;
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &scoped_graph);
        let index = TargetAnalysisIndex::rvs_build(&scoped_graph, &local_scope);
        let target_inference =
            NodeInference::rvs_from_prepared(&index, &analysis, &scoped_graph, &caps);
        let target_id = index
            .rvs_find_identity(&target)
            .expect("never: trait target belongs to the target index");
        let voted = target_inference.rvs_caps(target_id);
        let has_production_impl = rvs_node_slot(&index.vote_inputs, target_id)
            .iter()
            .flatten()
            .any(|implementation_id| {
                let implementation = index.rvs_target(*implementation_id);
                implementation.def_path == &second_impl_path
            });
        let output = format!(
            "caps={}\nhas_effectful_impl={}\n",
            voted.rvs_letters(),
            has_production_impl,
        );
        rvs_snapshot_BIS(
            "test_20260716_test_trait_vote_falls_back_to_production_implementation",
            &output,
        );

        assert_eq!(voted.rvs_letters(), "S");
        assert!(has_production_impl);
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
    fn test_20260811_incomplete_caps_warning_preserves_per_caller_anchors() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_first"),
            rvs_node(&["dependency::incomplete"]),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_second"),
            rvs_node(&["dependency::incomplete"]),
        );
        let mut info = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
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
        let anchor_callers: BTreeSet<String> = emission
            .span_anchors
            .iter()
            .map(|anchor| anchor.identity.def_path.rvs_as_str().to_string())
            .collect();
        let output = format!(
            "anchors={}\n",
            anchor_callers
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
        rvs_snapshot_BIS(
            "test_20260811_incomplete_caps_warning_preserves_per_caller_anchors",
            &output,
        );

        assert_eq!(emission.span_anchors.len(), 2);
        assert!(anchor_callers.contains("demo::rvs_first"));
        assert!(anchor_callers.contains("demo::rvs_second"));
    }

    #[test]
    fn test_20260811_incomplete_caps_same_caller_deduplicates_anchors() {
        let callee_path = DefPath::from("dependency::incomplete");
        let caller_path = DefPath::from("demo::rvs_caller");
        let mut node = rvs_node(&[callee_path.rvs_as_str()]);
        let callee_identity = FunctionIdentity {
            crate_id: 1,
            def_path: callee_path.clone(),
        };
        node.call_sites = BTreeSet::from([
            CallSiteIdentity {
                callee: callee_identity.clone(),
                occurrence: 0,
                source: Some(CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 10, 20)),
            },
            CallSiteIdentity {
                callee: callee_identity.clone(),
                occurrence: 1,
                source: Some(CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 30, 40)),
            },
            CallSiteIdentity {
                callee: callee_identity.clone(),
                occurrence: 2,
                source: Some(CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 50, 60)),
            },
        ]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(caller_path, node);
        let mut info = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Unknown,
        );
        info.rvs_with_source_M(CapabilitySource {
            layer: "deps".to_string(),
            file: PathBuf::from("caps/deps"),
            line: 2,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from(callee_path.rvs_as_str()), info);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::IncompleteCapsKnowledge)
            .expect("never: incomplete knowledge emits a warning");
        let output = format!("anchors={}\n", emission.span_anchors.len());
        rvs_snapshot_BIS(
            "test_20260811_incomplete_caps_same_caller_deduplicates_anchors",
            &output,
        );

        assert_eq!(emission.span_anchors.len(), 1);
    }

    #[test]
    fn test_20260729_incomplete_diagnostic_anchors_selected_target_call_site() {
        let callee_path = DefPath::from("dependency::incomplete");

        let mut sourceless = rvs_node(&[]);
        sourceless.crate_id = 9;
        sourceless.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 49,
                def_path: callee_path.clone(),
            },
            CallEdgeType::Strong,
        )]);
        sourceless.call_sites.clear();
        sourceless.sources =
            BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/aaa.rs"), 5, 18)]);
        sourceless.is_production = true;
        sourceless.is_coverage_candidate = true;

        let production_callee = FunctionIdentity {
            crate_id: 50,
            def_path: callee_path.clone(),
        };
        let first_path = DefPath::from("demo::rvs_alpha");
        let production_source = FnSource::rvs_new(PathBuf::from("src/alpha.rs"), 5, 14);
        let production_call_source = CallSiteSource::rvs_new(PathBuf::from("src/alpha.rs"), 40, 50);
        let mut first = rvs_node(&[]);
        first.crate_id = 10;
        first.calls = BTreeMap::from([(production_callee.clone(), CallEdgeType::Strong)]);
        first.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: production_callee.clone(),
            occurrence: 0,
            source: Some(production_call_source.clone()),
        }]);
        first.sources = BTreeSet::from([production_source]);
        first.is_production = true;
        first.is_coverage_candidate = true;

        let second_path = DefPath::from("demo::rvs_beta");
        let mut second = rvs_node(&[]);
        second.crate_id = 11;
        let second_callee = FunctionIdentity {
            crate_id: 51,
            def_path: callee_path.clone(),
        };
        second.calls = BTreeMap::from([(second_callee.clone(), CallEdgeType::Strong)]);
        second.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: second_callee,
            occurrence: 0,
            source: Some(CallSiteSource::rvs_new(
                PathBuf::from("src/beta.rs"),
                90,
                100,
            )),
        }]);
        second.sources = BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/beta.rs"), 5, 13)]);
        second.is_production = true;
        second.is_coverage_candidate = true;

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_aaa_no_site"), sourceless);
        graph.rvs_insert_M(first_path.clone(), first);
        graph.rvs_insert_M(second_path, second);
        let mut info = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Unknown,
        );
        info.rvs_with_source_M(CapabilitySource {
            layer: "deps".to_string(),
            file: PathBuf::from("caps/deps"),
            line: 2,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from(callee_path.rvs_as_str()), info);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let diagnostics = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::IncompleteCapsKnowledge)
            .collect::<Vec<_>>();
        let emission = report
            .rvs_emissions(&graph)
            .into_iter()
            .find(|emission| emission.lint == OfflineCapsLint::IncompleteCapsKnowledge)
            .expect("never: incomplete knowledge emits one aggregate warning");
        let first_anchor = emission
            .span_anchors
            .iter()
            .find(|anchor| anchor.identity.crate_id == 10 && anchor.identity.def_path == first_path)
            .expect("never: first production caller has an anchor");
        let anchor_caller_count = emission
            .span_anchors
            .iter()
            .map(|anchor| &anchor.identity)
            .collect::<BTreeSet<_>>()
            .len();
        let output = format!(
            "diagnostics={}\nanchor_count={}\nfirst_anchor_caller={}:{}\nfirst_anchor_callee={}:{}\nfirst_anchor_source={:?}\nexact_identity_detail={}\nwhy_context={}\n",
            diagnostics.len(),
            emission.span_anchors.len(),
            first_anchor.identity.crate_id,
            first_anchor.identity.def_path,
            first_anchor
                .call_site
                .as_ref()
                .map_or(0, |call_site| call_site.callee.crate_id),
            first_anchor
                .call_site
                .as_ref()
                .map_or("<none>", |call_site| call_site.callee.def_path.rvs_as_str()),
            first_anchor
                .call_site
                .as_ref()
                .and_then(|call_site| call_site.source.as_ref())
                .map(|source| &source.file),
            diagnostics[0]
                .details
                .iter()
                .any(|detail| detail.contains("crate_id=50")),
            diagnostics[0]
                .details
                .iter()
                .any(|detail| detail.contains("cargo rivus why 'dependency::incomplete' .")),
        );
        rvs_snapshot_BIS(
            "test_20260729_incomplete_diagnostic_anchors_selected_target_call_site",
            &output,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(emission.span_anchors.len(), 3);
        assert_eq!(anchor_caller_count, 3);
        assert_eq!(first_anchor.identity.crate_id, 10);
        assert_eq!(first_anchor.identity.def_path, first_path);
        assert_eq!(first_anchor.call_site, None);
        assert!(
            diagnostics[0]
                .details
                .iter()
                .any(|detail| detail.contains("crate_id=50"))
        );
    }

    #[test]
    fn test_20260729_target_filtering_precedes_diagnostic_grouping() {
        let effect_path = DefPath::from("demo::Service::effect");
        let effectful_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut effect = rvs_node(&[]);
        effect.facts = effectful_facts;
        effect.sources =
            BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/service.rs"), 5, 11)]);
        effect.is_production = true;

        let unknown_path = DefPath::from("dependency::unknown");
        let incomplete_path = DefPath::from("dependency::incomplete");
        let calls = [
            FunctionIdentity {
                crate_id: 1,
                def_path: effect_path.clone(),
            },
            FunctionIdentity {
                crate_id: 70,
                def_path: unknown_path.clone(),
            },
            FunctionIdentity {
                crate_id: 80,
                def_path: incomplete_path.clone(),
            },
        ];
        let caller_path = DefPath::from("demo::rvs_run");
        let mut caller = rvs_node(&[]);
        caller.crate_id = 10;
        caller.calls = calls
            .iter()
            .cloned()
            .map(|c| (c, CallEdgeType::Strong))
            .collect();
        caller.call_sites = calls
            .iter()
            .enumerate()
            .map(|(occurrence, callee)| CallSiteIdentity {
                callee: callee.clone(),
                occurrence: u32::try_from(occurrence)
                    .expect("never: regression has three production calls"),
                source: Some(CallSiteSource::rvs_new(
                    PathBuf::from("src/run.rs"),
                    20 + u32::try_from(occurrence).expect("never: small occurrence") * 10,
                    25 + u32::try_from(occurrence).expect("never: small occurrence") * 10,
                )),
            })
            .collect();
        caller.sources = BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/run.rs"), 5, 12)]);
        caller.is_production = true;
        caller.is_coverage_candidate = true;

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(effect_path, effect);
        graph.rvs_insert_M(caller_path.clone(), caller);
        let mut incomplete = CapabilityInfo::rvs_new(
            CapabilitySet::rvs_new(),
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Incomplete,
        );
        incomplete.rvs_with_source_M(CapabilitySource {
            layer: "deps".to_string(),
            file: PathBuf::from("caps/deps"),
            line: 9,
        });
        let mut caps = CapsMap::rvs_new();
        caps.rvs_insert_info_M(CapsMapKey::from(incomplete_path.rvs_as_str()), incomplete);

        let report =
            rvs_check_offline_caps(&graph, &caps, &BTreeSet::from([CrateName::from("demo")]));
        let emissions = report.rvs_emissions(&graph);
        let selected = emissions
            .iter()
            .filter(|emission| {
                matches!(
                    emission.lint,
                    OfflineCapsLint::CallViolation
                        | OfflineCapsLint::UnknownCallee
                        | OfflineCapsLint::IncompleteCapsKnowledge
                        | OfflineCapsLint::MissingSideEffect
                        | OfflineCapsLint::StaticRef
                )
            })
            .collect::<Vec<_>>();
        let anchors = selected
            .iter()
            .flat_map(|emission| emission.span_anchors.iter())
            .collect::<Vec<_>>();
        let call_violation = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::CallViolation)
            .expect("never: caller lacks the effect callee's S capability");
        let has_missing_side_effect = report.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == OfflineCapsKind::Contract(FnContractMismatchKind::MissingSideEffect)
                && diagnostic.span_anchors.values().flatten().copied().eq([10])
        });
        let output = format!(
            "selected_anchors={}\nall_caller={}\nall_caller_sources={}\ncall_caps_s={}\nmissing_side_effect={has_missing_side_effect}\n",
            anchors.len(),
            anchors.iter().all(|anchor| anchor.identity.crate_id == 10),
            anchors.iter().all(|anchor| {
                anchor.call_site.as_ref().is_none_or(|call_site| {
                    call_site
                        .source
                        .as_ref()
                        .is_some_and(|source| source.file == PathBuf::from("src/run.rs"))
                })
            }),
            call_violation
                .details
                .iter()
                .any(|detail| detail == "callee caps: S"),
        );
        rvs_snapshot_BIS(
            "test_20260729_target_filtering_precedes_diagnostic_grouping",
            &output,
        );

        assert!(anchors.iter().all(|anchor| anchor.identity.crate_id == 10));
        assert!(anchors.iter().all(|anchor| {
            anchor.call_site.as_ref().is_none_or(|call_site| {
                call_site
                    .source
                    .as_ref()
                    .is_some_and(|source| source.file == PathBuf::from("src/run.rs"))
            })
        }));
        assert!(
            call_violation
                .details
                .iter()
                .any(|detail| detail == "callee caps: S")
        );
        assert!(has_missing_side_effect);
    }

    #[test]
    fn test_20260729_trait_outlier_uses_selected_target_role() {
        let trait_path = DefPath::from("demo::Parser::rvs_parse");
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(trait_path, declaration);
        for (name, crate_id) in [("Alpha", 31), ("Beta", 32)] {
            let mut implementation = rvs_node(&[]);
            implementation.is_trait_impl = true;
            implementation.crate_id = crate_id;
            implementation.sources = BTreeSet::from([FnSource::rvs_new(
                PathBuf::from(format!("src/{name}.rs")),
                5,
                10,
            )]);
            implementation.is_production = true;
            graph.rvs_insert_M(
                DefPath::from(format!("demo::{name}::rvs_parse@demo::Parser")),
                implementation,
            );
        }

        let mixed_path = DefPath::from("demo::Mixed::rvs_parse@demo::Parser");
        let effect_call = FunctionIdentity {
            crate_id: 90,
            def_path: DefPath::from("dependency::effect"),
        };
        let mut mixed = rvs_node(&[]);
        mixed.is_trait_impl = true;
        mixed.crate_id = 10;
        mixed.calls = BTreeMap::from([(effect_call.clone(), CallEdgeType::Strong)]);
        mixed.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: effect_call,
            occurrence: 0,
            source: Some(CallSiteSource::rvs_new(
                PathBuf::from("src/mixed.rs"),
                30,
                40,
            )),
        }]);
        mixed.sources = BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/mixed.rs"), 5, 14)]);
        mixed.is_production = true;
        graph.rvs_insert_M(mixed_path, mixed);

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dependency::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let outliers = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::TraitImplOutlier)
            .collect::<Vec<_>>();
        let output = format!(
            "outliers={}\nproduction_anchor={}\n",
            outliers.len(),
            outliers.iter().any(|diagnostic| diagnostic
                .span_anchors
                .values()
                .any(|crate_ids| crate_ids.contains(&10))),
        );
        rvs_snapshot_BIS(
            "test_20260729_trait_outlier_uses_selected_target_role",
            &output,
        );

        assert_eq!(outliers.len(), 1);
        assert!(outliers.iter().any(|diagnostic| {
            diagnostic
                .span_anchors
                .values()
                .any(|crate_ids| crate_ids.contains(&10))
        }));
    }

    #[test]
    fn test_20260730_missing_target_does_not_borrow_same_path_caps() {
        let effect_path = DefPath::from("dependency::effect");
        let effect_facts = CapabilityFacts {
            has_static_ref: true,
            ..CapabilityFacts::default()
        };
        let mut effect = rvs_node(&[]);
        effect.facts = effect_facts;
        rvs_set_target_crates_M(&mut effect, &[50]);
        let effect_target = effect.rvs_test_target_M(50);
        effect_target.facts = effect_facts;
        effect_target.crate_provenance = CrateProvenance::Dependency;

        let caller_path = DefPath::from("demo::rvs_call");
        let missing_effect = FunctionIdentity {
            crate_id: 60,
            def_path: effect_path.clone(),
        };
        let mut caller = rvs_node(&[]);
        let caller_target = caller.rvs_test_target_M(1);
        caller_target.calls = BTreeMap::from([(missing_effect.clone(), CallEdgeType::Strong)]);
        caller_target.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: missing_effect,
            occurrence: 0,
            source: None,
        }]);

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(effect_path, effect);
        graph.rvs_insert_M(caller_path.clone(), caller);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let caps = CapsMap::rvs_new();
        let analysis = PreparedLocalAnalysis::rvs_prepare(&graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &graph);
        let index = TargetAnalysisIndex::rvs_build(&graph, &local_scope);
        let inference = NodeInference::rvs_from_prepared(&index, &analysis, &graph, &caps);
        let caller_id = index
            .rvs_find_target(&caller_path, 1)
            .expect("never: caller target belongs to the index");
        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let has_call_violation = report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == OfflineCapsKind::CallViolation);
        let has_unknown = report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee);
        let output = format!(
            "caps={}\nincomplete={}\ncall_violation={has_call_violation}\nunknown={has_unknown}\n",
            rvs_format_caps(inference.rvs_caps(caller_id)),
            inference.rvs_is_incomplete(caller_id),
        );
        rvs_snapshot_BIS(
            "test_20260730_missing_target_does_not_borrow_same_path_caps",
            &output,
        );

        assert!(has_call_violation);
        assert!(!has_unknown);
    }

    #[test]
    fn test_20260730_bodyless_target_without_boundary_stays_unknown() {
        let opaque_path = DefPath::from("dependency::opaque");
        let mut opaque = rvs_node(&[]);
        opaque.has_body = false;
        rvs_set_target_crates_M(&mut opaque, &[50]);
        let opaque_target = opaque.rvs_test_target_M(50);
        opaque_target.has_body = false;
        opaque_target.crate_provenance = CrateProvenance::Dependency;

        let caller_path = DefPath::from("demo::rvs_call");
        let opaque_identity = FunctionIdentity {
            crate_id: 50,
            def_path: opaque_path.clone(),
        };
        let mut caller = rvs_node(&[]);
        let caller_target = caller.rvs_test_target_M(1);
        caller_target.calls = BTreeMap::from([(opaque_identity.clone(), CallEdgeType::Strong)]);
        caller_target.call_sites = BTreeSet::from([CallSiteIdentity {
            callee: opaque_identity,
            occurrence: 0,
            source: None,
        }]);

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(opaque_path.clone(), opaque);
        graph.rvs_insert_M(caller_path, caller);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let caps = CapsMap::rvs_new();
        let analysis = PreparedLocalAnalysis::rvs_prepare(&graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &graph);
        let index = TargetAnalysisIndex::rvs_build(&graph, &local_scope);
        let inference = NodeInference::rvs_from_prepared(&index, &analysis, &graph, &caps);
        let opaque_id = index
            .rvs_find_target(&opaque_path, 50)
            .expect("never: bodyless target belongs to the index");
        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let incomplete_warnings = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::IncompleteCapsKnowledge)
            .count();
        let has_unknown = report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee);
        let output = format!(
            "caps={}\nincomplete={}\nincomplete_warnings={incomplete_warnings}\nunknown={has_unknown}\n",
            rvs_format_caps(inference.rvs_caps(opaque_id)),
            inference.rvs_is_incomplete(opaque_id),
        );
        rvs_snapshot_BIS(
            "test_20260730_bodyless_target_without_boundary_stays_unknown",
            &output,
        );

        assert!(inference.rvs_caps(opaque_id).rvs_is_empty());
        assert!(inference.rvs_is_incomplete(opaque_id));
        assert_eq!(incomplete_warnings, 1);
        assert!(has_unknown);
    }

    #[test]
    fn test_20260730_complete_exact_boundary_terminates_incompleteness() {
        let effect_path = DefPath::from("dependency::effect");
        let mut effect = rvs_node(&[]);
        effect.crate_id = 50;
        effect.crate_provenance = CrateProvenance::Dependency;

        let caller_path = DefPath::from("demo::rvs_call_S");
        let mut caller = rvs_node(&[]);
        caller.crate_id = 1;
        caller.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 50,
                def_path: effect_path.clone(),
            },
            CallEdgeType::Strong,
        )]);
        caller.call_sites.clear();

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(effect_path.clone(), effect);
        graph.rvs_insert_M(caller_path.clone(), caller);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let caps = rvs_make_capsmap(&[(effect_path.rvs_as_str(), "S")]);
        let analysis = PreparedLocalAnalysis::rvs_prepare(&graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &graph);
        let index = TargetAnalysisIndex::rvs_build(&graph, &local_scope);
        let inference = NodeInference::rvs_from_prepared(&index, &analysis, &graph, &caps);
        let caller_id = index
            .rvs_find_target(&caller_path, 1)
            .expect("never: caller target belongs to the index");
        let output = format!(
            "caps={}\nincomplete={}\n",
            rvs_format_caps(inference.rvs_caps(caller_id)),
            inference.rvs_is_incomplete(caller_id),
        );
        rvs_snapshot_BIS(
            "test_20260730_complete_exact_boundary_terminates_incompleteness",
            &output,
        );

        assert_eq!(inference.rvs_caps(caller_id).rvs_letters(), "S");
        assert!(!inference.rvs_is_incomplete(caller_id));
    }

    #[test]
    fn test_20260814_node_incomplete_agrees_with_prepared_incomplete_paths() {
        let local_path = DefPath::from("demo::rvs_wrapped");
        let mut local_node = rvs_node(&["dependency::opaque"]);
        local_node.crate_id = 1;
        let mut dependency = rvs_node(&[]);
        dependency.has_body = false;
        dependency.crate_id = 50;
        dependency.crate_provenance = CrateProvenance::Dependency;

        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(local_path.clone(), local_node);
        graph.rvs_insert_M(DefPath::from("dependency::opaque"), dependency);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let caps = CapsMap::rvs_new();
        let analysis = PreparedLocalAnalysis::rvs_prepare(&graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &graph);
        let index = TargetAnalysisIndex::rvs_build(&graph, &local_scope);
        let inference = NodeInference::rvs_from_prepared(&index, &analysis, &graph, &caps);
        let local_id = index
            .rvs_find_target(&local_path, 1)
            .expect("never: local target belongs to the index");
        let node_incomplete = inference.rvs_is_incomplete(local_id);
        let prepared_incomplete = analysis.rvs_incomplete_paths().contains(&local_path);
        let output = format!(
            "node_incomplete={node_incomplete}\nprepared_incomplete={prepared_incomplete}\n"
        );
        rvs_snapshot_BIS(
            "test_20260814_node_incomplete_agrees_with_prepared_incomplete_paths",
            &output,
        );

        assert_eq!(node_incomplete, prepared_incomplete);
        assert!(
            prepared_incomplete,
            "opaque callee taints the local wrapper"
        );
    }

    #[test]
    fn test_20260730_trait_outlier_grouping_has_bounded_comparisons() {
        const GROUP_COUNT: usize = 128;
        let mut graph = FnGraph::rvs_new();
        for group in 0..GROUP_COUNT {
            let trait_path = DefPath::from(format!("stress::Trait{group:04}::rvs_run"));
            let mut declaration = rvs_node(&[]);
            declaration.has_body = false;
            graph.rvs_insert_M(trait_path, declaration);
            for implementation in 0..3 {
                let calls = if implementation == 2 {
                    &["dependency::effect"][..]
                } else {
                    &[][..]
                };
                let mut node = rvs_node(calls);
                node.is_trait_impl = true;
                graph.rvs_insert_M(
                    DefPath::from(format!(
                        "stress::Type{group:04}_{implementation}::rvs_run@stress::Trait{group:04}"
                    )),
                    node,
                );
            }
        }
        let local = BTreeSet::from([CrateName::from("stress")]);
        let caps = rvs_make_capsmap(&[("dependency::effect", "S")]);
        let analysis = PreparedLocalAnalysis::rvs_prepare(&graph, &caps, &local);
        let groups = analysis.trait_impl_outliers;
        let output = format!("groups={}\n", groups.len());
        rvs_snapshot_BIS(
            "test_20260730_trait_outlier_grouping_has_bounded_comparisons",
            &output,
        );

        assert_eq!(groups.len(), GROUP_COUNT);
    }

    #[test]
    fn test_20260729_target_analysis_branching_cycle_has_bounded_work() {
        const NODE_COUNT: usize = 256;
        const EDGE_COUNT: usize = NODE_COUNT * 2;
        let mut graph = FnGraph::rvs_new();
        for index in 0..NODE_COUNT {
            let next = (index + 1) % NODE_COUNT;
            let branch = (index + 2) % NODE_COUNT;
            let mut node = rvs_node(&[]);
            node.calls = BTreeMap::from([
                (
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(format!("stress::rvs_node_{next:04}")),
                    },
                    CallEdgeType::Strong,
                ),
                (
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(format!("stress::rvs_node_{branch:04}")),
                    },
                    CallEdgeType::Strong,
                ),
            ]);
            if index + 1 == NODE_COUNT {
                node.facts.has_static_ref = true;
            }
            graph.rvs_insert_M(DefPath::from(format!("stress::rvs_node_{index:04}")), node);
        }
        let local = BTreeSet::from([CrateName::from("stress")]);
        let mut scoped_graph = graph;
        let caps = CapsMap::rvs_new();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut scoped_graph, &caps, &local);
        let local_scope = LocalScope::rvs_for_graph(&local, &scoped_graph);
        let index = TargetAnalysisIndex::rvs_build(&scoped_graph, &local_scope);
        let target_inference =
            NodeInference::rvs_from_prepared(&index, &analysis, &scoped_graph, &caps);
        let first_identity = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("stress::rvs_node_0000"),
        };
        let first = target_inference
            .rvs_caps_for_identity(&index, &first_identity)
            .expect("never: inference covers every synthetic target")
            .rvs_letters();
        let output = format!("nodes={NODE_COUNT}\nedges={EDGE_COUNT}\nfirst_caps={first}\n",);
        rvs_snapshot_BIS(
            "test_20260729_target_analysis_branching_cycle_has_bounded_work",
            &output,
        );

        assert_eq!(first, "S");
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
            }]),
            message: "missing S capability".to_string(),
        }];

        let validated = rvs_validate_emissions(&emissions).unwrap();
        let json = rvs_serialize_emissions(&emissions).unwrap();
        let parsed = rvs_parse_emissions(&json).unwrap();
        rvs_snapshot_BIS(
            "test_20260715_offline_caps_emissions_round_trip",
            &(json + "\n"),
        );

        assert_eq!(validated, emissions.as_slice());
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
    fn test_20260716_unknown_callee_preserves_specialized_identity() {
        let known_path = DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c75383e}::run");
        let unknown_path = DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c7531363e}::run");
        let known = FunctionIdentity {
            crate_id: 2,
            def_path: known_path.clone(),
        };
        let unknown = FunctionIdentity {
            crate_id: 2,
            def_path: unknown_path.clone(),
        };
        let mut node = rvs_node(&[]);
        node.calls = BTreeMap::from([
            (known.clone(), CallEdgeType::Strong),
            (unknown.clone(), CallEdgeType::Strong),
        ]);
        let target = node.rvs_test_target_M(1);
        target.calls = BTreeMap::from([
            (known.clone(), CallEdgeType::Strong),
            (unknown.clone(), CallEdgeType::Strong),
        ]);
        target.call_sites = BTreeSet::from([
            CallSiteIdentity {
                callee: known,
                occurrence: 0,
                source: None,
            },
            CallSiteIdentity {
                callee: unknown.clone(),
                occurrence: 1,
                source: None,
            },
        ]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_handle"), node);
        let caps = rvs_make_capsmap(&[(known_path.rvs_as_str(), "")]);
        let local = BTreeSet::from([CrateName::from("demo")]);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == OfflineCapsKind::UnknownCallee)
            .expect("never: unknown specialization produces a diagnostic");
        let exact_unknown = diagnostic
            .call_site_anchors
            .iter()
            .all(|anchor| anchor.call_site.callee == unknown);
        let rendered = report.to_string();
        let output = format!(
            "anchor_count={}\nexact_unknown={exact_unknown}\nspan_fallback={}\nrendered_marker_free={}\n",
            diagnostic.call_site_anchors.len(),
            !diagnostic.span_anchors.is_empty(),
            !rendered.contains("{impl#"),
        );
        rvs_snapshot_BIS(
            "test_20260716_unknown_callee_preserves_specialized_identity",
            &output,
        );

        assert_eq!(diagnostic.call_site_anchors.len(), 1);
        assert!(exact_unknown);
        assert!(diagnostic.span_anchors.is_empty());
        assert!(!rendered.contains("{impl#"));
    }

    #[test]
    fn test_20260715_offline_unknown_callees_group_callers_by_callee() {
        let mut graph = FnGraph::rvs_new();
        let callee_identity = FunctionIdentity {
            crate_id: 2,
            def_path: DefPath::from("dep::plain"),
        };
        for name in ["demo::rvs_first", "demo::rvs_second"] {
            let mut node = rvs_node(&[]);
            node.calls = BTreeMap::from([(callee_identity.clone(), CallEdgeType::Strong)]);
            node.call_sites = BTreeSet::from([CallSiteIdentity {
                callee: callee_identity.clone(),
                occurrence: 0,
                source: None,
            }]);
            graph.rvs_insert_M(DefPath::from(name), node);
        }
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
        let callee_identity = FunctionIdentity {
            crate_id: 2,
            def_path: DefPath::from("dep::plain"),
        };
        for path in [
            "demo::Worker{impl#7538}::rvs_call",
            "demo::Worker{impl#753136}::rvs_call",
        ] {
            let mut node = rvs_node(&[]);
            node.calls = BTreeMap::from([(callee_identity.clone(), CallEdgeType::Strong)]);
            node.call_sites = BTreeSet::from([CallSiteIdentity {
                callee: callee_identity.clone(),
                occurrence: 0,
                source: None,
            }]);
            graph.rvs_insert_M(DefPath::from(path), node);
        }
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
        app.facts.is_port_method = true;
        let mut dependency = rvs_node(&[]);
        dependency.facts.is_port_method = true;
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
    fn test_20260801_world_port_impl_hides_concrete_effects() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        declaration.facts.is_port_method = true;
        let declaration_target = declaration.rvs_test_target_M(1);
        declaration_target.has_body = false;
        declaration_target.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::Transport::rvs_fetch_PS"), declaration);

        let mut node = rvs_node(&["dep::effect"]);
        node.is_trait_impl = true;
        node.facts.is_port_method = true;
        let facts = node.facts;
        let target = node.rvs_test_target_M(1);
        target.is_trait_impl = true;
        target.facts = facts;
        graph.rvs_insert_M(
            DefPath::from("demo::Adapter::rvs_fetch_PS@demo::Transport"),
            node,
        );
        let caps = rvs_make_capsmap(&[("dep::effect", "S")]);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut analysis_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut analysis_graph, &caps, &local);

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let contract_mismatches: usize = analysis
            .diffs
            .iter()
            .map(|diff| diff.rvs_mismatch_kinds().len())
            .sum();
        let output = format!("contract_mismatches={contract_mismatches}\n{report}");
        rvs_snapshot_BIS(
            "test_20260801_world_port_impl_hides_concrete_effects",
            &output,
        );

        assert_eq!(contract_mismatches, 0);
        assert!(
            !report
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
    fn test_20260806_world_port_votes_effects_but_caller_requires_only_p() {
        let port_path = DefPath::from("demo::Transport::rvs_fetch_BIPS");
        let mut graph = FnGraph::rvs_new();

        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        declaration.facts.is_port_method = true;
        let declaration_target = declaration.rvs_test_target_M(1);
        declaration_target.has_body = false;
        declaration_target.facts.is_port_method = true;
        graph.rvs_insert_M(port_path.clone(), declaration);

        let mut implementation = rvs_node(&["dep::effect"]);
        implementation.is_trait_impl = true;
        implementation.facts.is_port_method = true;
        let implementation_target = implementation.rvs_test_target_M(1);
        implementation_target.is_trait_impl = true;
        implementation_target.facts.is_port_method = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Adapter::rvs_fetch_BIPS@demo::Transport"),
            implementation,
        );

        graph.rvs_insert_M(
            DefPath::from("demo::rvs_use_P"),
            rvs_node(&["demo::Transport::rvs_fetch_BIPS"]),
        );

        let caps = rvs_make_capsmap(&[("dep::effect", "BIS")]);
        let local = BTreeSet::from([CrateName::from("demo")]);
        let mut analysis_graph = graph.clone();
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut analysis_graph, &caps, &local);
        let port_caps = analysis
            .rvs_inferred()
            .get(&port_path)
            .expect("never: World Port operation has inferred capabilities")
            .rvs_letters();
        let caller_caps = analysis
            .rvs_inferred()
            .get(&DefPath::from("demo::rvs_use_P"))
            .expect("never: World Port caller has inferred capabilities")
            .rvs_letters();
        let contract_mismatches: usize = analysis
            .diffs
            .iter()
            .map(|diff| diff.rvs_mismatch_kinds().len())
            .sum();

        let report = rvs_check_offline_caps(&graph, &caps, &local);
        let output = format!(
            "port_caps={port_caps}\ncaller_caps={caller_caps}\ncontract_mismatches={contract_mismatches}\n{}",
            report,
        );
        rvs_snapshot_BIS(
            "test_20260806_world_port_votes_effects_but_caller_requires_only_p",
            &output,
        );

        assert_eq!(port_caps, "BIPS");
        assert_eq!(caller_caps, "P");
        assert_eq!(contract_mismatches, 0);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == OfflineCapsKind::CallViolation)
        );
    }

    #[test]
    fn test_20260806_world_port_rejects_unvoted_implementation_effect() {
        let mut graph = FnGraph::rvs_new();
        let mut declaration = rvs_node(&[]);
        declaration.has_body = false;
        declaration.facts.is_port_method = true;
        let declaration_target = declaration.rvs_test_target_M(1);
        declaration_target.has_body = false;
        declaration_target.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::Transport::rvs_fetch_P"), declaration);

        for (adapter, calls) in [
            ("EffectAdapter", &["dep::effect"][..]),
            ("PureAdapterA", &[][..]),
            ("PureAdapterB", &[][..]),
        ] {
            let mut implementation = rvs_node(calls);
            implementation.is_trait_impl = true;
            implementation.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("demo::{adapter}::rvs_fetch_P@demo::Transport")),
                implementation,
            );
        }

        let report = rvs_check_offline_caps(
            &graph,
            &rvs_make_capsmap(&[("dep::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260806_world_port_rejects_unvoted_implementation_effect",
            &output,
        );

        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::CallViolation)
                .count(),
            1
        );
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == OfflineCapsKind::TraitImplOutlier)
        );
    }

    #[test]
    fn test_20260801_world_port_impl_allows_environment_state_but_not_static_mut() {
        let mut graph = FnGraph::rvs_new();
        for (
            operation_path,
            implementation_path,
            has_static_ref,
            has_static_mut_ref,
            has_thread_local_ref,
        ) in [
            (
                "demo::Transport::rvs_read_PST",
                "demo::Adapter::rvs_read_PST@demo::Transport",
                true,
                false,
                true,
            ),
            (
                "demo::Transport::rvs_write_PS",
                "demo::Adapter::rvs_write_PS@demo::Transport",
                false,
                true,
                false,
            ),
        ] {
            let mut declaration = rvs_node(&[]);
            declaration.has_body = false;
            declaration.facts.is_port_method = true;
            let declaration_target = declaration.rvs_test_target_M(1);
            declaration_target.has_body = false;
            declaration_target.facts.is_port_method = true;
            graph.rvs_insert_M(DefPath::from(operation_path), declaration);

            let mut node = rvs_node(&[]);
            node.is_trait_impl = true;
            node.facts = CapabilityFacts {
                has_static_ref,
                has_static_mut_ref,
                has_thread_local_ref,
                is_port_method: true,
                ..CapabilityFacts::default()
            };
            let facts = node.facts;
            let target = node.rvs_test_target_M(1);
            target.is_trait_impl = true;
            target.facts = facts;
            graph.rvs_insert_M(DefPath::from(implementation_path), node);
        }
        let report = rvs_check_offline_caps(
            &graph,
            &CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = report.to_string();
        rvs_snapshot_BIS(
            "test_20260801_world_port_impl_allows_environment_state_but_not_static_mut",
            &output,
        );

        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == OfflineCapsKind::StaticRefRequiresCaps)
                .count(),
            1
        );
        assert!(output.contains("missing: U"));
        assert!(!output.contains("rvs_read_PST"));
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
        let mut transitive = rvs_node(&[]);
        transitive.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_transitive_helper"),
            },
            CallEdgeType::Strong,
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_transitive"), transitive);
        graph.rvs_insert_M(DefPath::from("demo::rvs_transitive_helper"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::rvs_name_fallback"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::one::rvs_ambiguous"), rvs_node(&[]));
        graph.rvs_insert_M(DefPath::from("demo::two::rvs_ambiguous"), rvs_node(&[]));
        let mut cfg_wrapper = rvs_node(&[]);
        cfg_wrapper.calls = BTreeMap::from([(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_cfg_test_only"),
            },
            CallEdgeType::Strong,
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_cfg_wrapper"), cfg_wrapper);
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_cfg_production_only"),
            rvs_node(&[]),
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_cfg_test_only"), rvs_node(&[]));
        let mut generated = rvs_node(&[]);
        generated.sources.clear();
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
        test_only_helper.is_coverage_candidate = false;
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_test_only_helper"),
            test_only_helper,
        );

        let mut test_node = rvs_node(&[]);
        test_node.is_test = true;
        test_node.is_test_compilation = true;
        test_node.is_production = false;
        test_node.is_coverage_candidate = false;
        test_node.calls = BTreeMap::from([
            (
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_covered"),
                },
                CallEdgeType::Strong,
            ),
            (
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_transitive"),
                },
                CallEdgeType::Strong,
            ),
            (
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_generated"),
                },
                CallEdgeType::Strong,
            ),
            (
                FunctionIdentity {
                    crate_id: 1,
                    def_path: DefPath::from("demo::rvs_cfg_wrapper"),
                },
                CallEdgeType::Strong,
            ),
        ]);
        test_node
            .unresolved_test_calls
            .insert("rvs_name_fallback".to_string());
        test_node
            .unresolved_test_calls
            .insert("rvs_ambiguous".to_string());
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
