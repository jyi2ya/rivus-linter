use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::artifacts::{FnGraph, FnNode, FunctionIdentity};
use crate::capability::{
    Capability, CapabilityBasis, CapabilityCompleteness, CapabilityInfo, CapabilityPolicy,
    CapabilitySet, ParsedFunctionName, rvs_parse_function,
};
use crate::capsmap;
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::symbols::{CapsMapKey, CrateName, DefPath, FnName, TraitMethodKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FnContractDiff {
    pub(crate) def_path: DefPath,
    pub(crate) actual_name: FnName,
    pub(crate) expected_name: FnName,
    pub(crate) declared_public_caps: Option<CapabilitySet>,
    pub(crate) expected_public_caps: CapabilitySet,
}

#[derive(Debug)]
pub(crate) struct PreparedInference {
    inferred: BTreeMap<DefPath, CapabilitySet>,
    impl_index: HashMap<TraitMethodKey, Vec<DefPath>>,
    scoped_port_methods: BTreeSet<DefPath>,
    synthetic_paths: BTreeSet<DefPath>,
    incomplete_paths: BTreeSet<DefPath>,
    trait_votes: BTreeMap<DefPath, TraitCapabilityVote>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraitVoteImplementation {
    pub(crate) path: DefPath,
    pub(crate) propagated_caps: CapabilitySet,
    pub(crate) incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraitCapabilityVote {
    pub(crate) trait_method: DefPath,
    pub(crate) selected_caps: CapabilitySet,
    pub(crate) implementations: Vec<TraitVoteImplementation>,
    pub(crate) threshold: usize,
    pub(crate) counts: BTreeMap<Capability, usize>,
    pub(crate) is_port: bool,
}

impl TraitCapabilityVote {
    pub(crate) fn rvs_is_complete(&self) -> bool {
        self.implementations
            .iter()
            .all(|implementation| !implementation.incomplete)
    }

    #[cfg(test)]
    pub(crate) fn rvs_capability_info(&self) -> CapabilityInfo {
        self.rvs_capability_info_with_lower_bound(&self.selected_caps, false)
    }

    fn rvs_capability_info_with_lower_bound(
        &self,
        lower_bound: &CapabilitySet,
        trait_method_incomplete: bool,
    ) -> CapabilityInfo {
        let mut combined = self.selected_caps.clone();
        let _ = combined.rvs_extend_filtered_M(lower_bound, |_| true);
        let completeness = if !trait_method_incomplete && self.rvs_is_complete() {
            CapabilityCompleteness::Complete
        } else {
            CapabilityCompleteness::Incomplete
        };
        if combined == self.selected_caps {
            CapabilityInfo::rvs_trait_vote(
                combined,
                self.implementations.len(),
                self.threshold,
                self.counts.clone(),
                completeness,
            )
        } else {
            CapabilityInfo::rvs_new(combined, CapabilityBasis::Inferred, completeness)
        }
    }
}

impl PreparedInference {
    pub(crate) fn rvs_prepare(
        graph: &FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        let scoped_port_methods = rvs_scoped_port_methods(graph, local_crate_names);
        let impl_index = rvs_build_impl_index(graph);
        let dependents = rvs_build_inference_dependents(graph, &impl_index);
        let inferred = rvs_infer_caps_with_knowledge_and_ports(
            graph,
            CapabilityKnowledgeView::rvs_base(seed),
            &impl_index,
            &scoped_port_methods,
            &dependents,
        );
        let synthetic_paths = inferred
            .keys()
            .filter(|path| graph.rvs_get(path.rvs_as_str()).is_none())
            .cloned()
            .collect();
        let incomplete_paths = rvs_incomplete_inference_paths_with_knowledge(
            graph,
            CapabilityKnowledgeView::rvs_base(seed),
            &inferred,
            &impl_index,
            &scoped_port_methods,
            &dependents,
        );
        let trait_votes = rvs_collect_trait_votes_with_ports(
            graph,
            &impl_index,
            &inferred,
            &incomplete_paths,
            &scoped_port_methods,
        );
        Self {
            inferred,
            impl_index,
            scoped_port_methods,
            synthetic_paths,
            incomplete_paths,
            trait_votes,
        }
    }

    pub(crate) fn rvs_prepare_M(
        graph: &mut FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        Self::rvs_prepare(graph, seed, local_crate_names)
    }

    pub(crate) fn rvs_inferred(&self) -> &BTreeMap<DefPath, CapabilitySet> {
        &self.inferred
    }

    #[cfg(test)]
    pub(crate) fn rvs_impl_index(&self) -> &HashMap<TraitMethodKey, Vec<DefPath>> {
        &self.impl_index
    }

    pub(crate) fn rvs_synthetic_paths(&self) -> &BTreeSet<DefPath> {
        &self.synthetic_paths
    }

    pub(crate) fn rvs_incomplete_paths(&self) -> &BTreeSet<DefPath> {
        &self.incomplete_paths
    }

    pub(crate) fn rvs_trait_votes(&self) -> &BTreeMap<DefPath, TraitCapabilityVote> {
        &self.trait_votes
    }

    pub(crate) fn rvs_resolver<'a>(
        &'a self,
        graph: &'a FnGraph,
        seed: &'a capsmap::CapsMap,
    ) -> CalleeCapsResolver<'a> {
        CalleeCapsResolver::rvs_new_scoped(
            graph,
            seed,
            &self.inferred,
            &self.impl_index,
            &self.scoped_port_methods,
        )
    }

    pub(crate) fn rvs_collect_direct_external_deps(
        &self,
        graph: &FnGraph,
        local_crate_names: &BTreeSet<CrateName>,
        seed: &capsmap::CapsMap,
    ) -> (
        BTreeMap<DefPath, CapabilityInfo>,
        BTreeMap<DefPath, BTreeSet<DefPath>>,
    ) {
        let local_scope = LocalScope::rvs_for_graph(local_crate_names, graph);
        let mut known = BTreeMap::new();
        let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
        let resolver = self.rvs_resolver(graph, seed);
        let mut record_external = |func: &DefPath, callee: &DefPath| {
            if seed.rvs_lookup_def_path(callee).is_some() {
                return;
            }
            if let Some(caps) = resolver.rvs_for_propagation_target(callee) {
                let incomplete = self.incomplete_paths.contains(callee);
                let info = match self.trait_votes.get(callee) {
                    Some(vote) => vote.rvs_capability_info_with_lower_bound(&caps, incomplete),
                    None => CapabilityInfo::rvs_new(
                        caps,
                        CapabilityBasis::Inferred,
                        if incomplete {
                            CapabilityCompleteness::Incomplete
                        } else {
                            CapabilityCompleteness::Complete
                        },
                    ),
                };
                known.entry(callee.clone()).or_insert(info);
            } else {
                unknown
                    .entry(callee.clone())
                    .or_default()
                    .insert(func.clone());
            }
        };
        for (func, behavior) in graph.rvs_iter() {
            if !local_scope.rvs_contains_target(func, behavior.crate_provenance) {
                continue;
            }
            for callee in behavior.calls.keys() {
                if local_scope.rvs_contains_identity(callee) {
                    continue;
                }
                record_external(func, &callee.def_path);
            }
        }
        (known, unknown)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedLocalAnalysis {
    pub(crate) diffs: Vec<FnContractDiff>,
    pub(crate) trait_impl_outliers: Vec<TraitImplOutlier>,
    inference: PreparedInference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraitImplOutlier {
    pub(crate) trait_method: DefPath,
    pub(crate) implementation: DefPath,
    pub(crate) implementation_caps: CapabilitySet,
    pub(crate) selected_caps: CapabilitySet,
    pub(crate) unexpected_caps: CapabilitySet,
    pub(crate) implementations: usize,
    pub(crate) threshold: usize,
    pub(crate) counts: BTreeMap<Capability, usize>,
}

impl PreparedLocalAnalysis {
    pub(crate) fn rvs_prepare(
        graph: &FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        let inference = PreparedInference::rvs_prepare(graph, seed, local_crate_names);
        let diffs = rvs_collect_contract_diffs_with_incomplete(
            graph,
            inference.rvs_inferred(),
            local_crate_names,
            inference.rvs_incomplete_paths(),
        );
        let trait_impl_outliers = rvs_collect_local_trait_vote_outliers(
            graph,
            inference.rvs_trait_votes(),
            local_crate_names,
        );
        Self {
            diffs,
            trait_impl_outliers,
            inference,
        }
    }

    pub(crate) fn rvs_prepare_M(
        graph: &mut FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        Self::rvs_prepare(graph, seed, local_crate_names)
    }

    pub(crate) fn rvs_inferred(&self) -> &BTreeMap<DefPath, CapabilitySet> {
        self.inference.rvs_inferred()
    }

    pub(crate) fn rvs_synthetic_paths(&self) -> &BTreeSet<DefPath> {
        self.inference.rvs_synthetic_paths()
    }

    pub(crate) fn rvs_incomplete_paths(&self) -> &BTreeSet<DefPath> {
        self.inference.rvs_incomplete_paths()
    }

    pub(crate) fn rvs_trait_votes(&self) -> &BTreeMap<DefPath, TraitCapabilityVote> {
        self.inference.rvs_trait_votes()
    }

    pub(crate) fn rvs_resolver<'a>(
        &'a self,
        graph: &'a FnGraph,
        seed: &'a capsmap::CapsMap,
    ) -> CalleeCapsResolver<'a> {
        self.inference.rvs_resolver(graph, seed)
    }
}

fn rvs_collect_local_trait_vote_outliers(
    graph: &FnGraph,
    votes: &BTreeMap<DefPath, TraitCapabilityVote>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<TraitImplOutlier> {
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let mut outliers = Vec::new();
    for vote in votes.values() {
        if vote.is_port || !vote.rvs_is_complete() {
            continue;
        }
        for implementation in &vote.implementations {
            let Some(node) = graph.rvs_get(implementation.path.rvs_as_str()) else {
                continue;
            };
            if !FunctionClassification::rvs_new(&scope, &implementation.path, node)
                .rvs_is_trait_vote_outlier_candidate()
            {
                continue;
            }
            let mut unexpected_caps = CapabilitySet::rvs_new();
            let _ = unexpected_caps
                .rvs_extend_filtered_M(&implementation.propagated_caps, |capability| {
                    !vote.selected_caps.rvs_contains(capability)
                });
            if unexpected_caps.rvs_is_empty() {
                continue;
            }
            outliers.push(TraitImplOutlier {
                trait_method: vote.trait_method.clone(),
                implementation: implementation.path.clone(),
                implementation_caps: implementation.propagated_caps.clone(),
                selected_caps: vote.selected_caps.clone(),
                unexpected_caps,
                implementations: vote.implementations.len(),
                threshold: vote.threshold,
                counts: vote.counts.clone(),
            });
        }
    }
    outliers.sort_by(|left, right| left.implementation.cmp(&right.implementation));
    outliers
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FnContractMismatch {
    pub(crate) def_path: DefPath,
    pub(crate) actual_name: FnName,
    pub(crate) kind: FnContractMismatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallContractMismatchKind {
    UnknownCallee,
    MissingCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallContractMismatch {
    pub(crate) callee_display: String,
    pub(crate) kind: CallContractMismatchKind,
    pub(crate) callee_caps: Option<CapabilitySet>,
    pub(crate) missing_caps: BTreeSet<Capability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FnContractMismatchKind {
    MissingRvsPrefix,
    NameMismatch,
    MissingAsync,
    MissingBlocking,
    MissingIo,
    MissingMutable,
    MissingPort,
    MissingSideEffect,
    MissingThreadLocal,
    MissingUnsafe,
}

impl FnContractMismatchKind {
    pub(crate) fn rvs_as_str(self) -> &'static str {
        match self {
            Self::MissingRvsPrefix => "missing_rvs_prefix",
            Self::NameMismatch => "name_mismatch",
            Self::MissingAsync => "missing_async",
            Self::MissingBlocking => "missing_blocking",
            Self::MissingIo => "missing_io",
            Self::MissingMutable => "missing_mutable",
            Self::MissingPort => "missing_port",
            Self::MissingSideEffect => "missing_side_effect",
            Self::MissingThreadLocal => "missing_thread_local",
            Self::MissingUnsafe => "missing_unsafe",
        }
    }
}

impl FnContractDiff {
    pub(crate) fn rvs_has_name_mismatch(&self) -> bool {
        self.expected_name != self.actual_name
    }

    pub(crate) fn rvs_missing_rvs_prefix(&self) -> bool {
        !self.actual_name.rvs_as_str().starts_with("rvs_")
    }

    pub(crate) fn rvs_mismatch_kinds(&self) -> Vec<FnContractMismatchKind> {
        let mut mismatches = Vec::new();
        if self.rvs_missing_rvs_prefix() {
            mismatches.push(FnContractMismatchKind::MissingRvsPrefix);
        } else if self.rvs_has_name_mismatch() {
            mismatches.push(FnContractMismatchKind::NameMismatch);
        }
        let declared_has = |cap| {
            self.declared_public_caps
                .as_ref()
                .is_some_and(|caps| caps.rvs_contains(cap))
        };
        for (cap, kind) in [
            (Capability::A, FnContractMismatchKind::MissingAsync),
            (Capability::B, FnContractMismatchKind::MissingBlocking),
            (Capability::I, FnContractMismatchKind::MissingIo),
            (Capability::M, FnContractMismatchKind::MissingMutable),
            (Capability::P, FnContractMismatchKind::MissingPort),
            (Capability::S, FnContractMismatchKind::MissingSideEffect),
            (Capability::T, FnContractMismatchKind::MissingThreadLocal),
            (Capability::U, FnContractMismatchKind::MissingUnsafe),
        ] {
            if self.expected_public_caps.rvs_contains(cap) && !declared_has(cap) {
                mismatches.push(kind);
            }
        }
        mismatches
    }
}

pub(crate) fn rvs_collect_call_contract_mismatch(
    def_path: &str,
    caps: &CapabilitySet,
    callee_caps: Option<&CapabilitySet>,
) -> Option<CallContractMismatch> {
    let callee_display = def_path.to_string();
    let Some(callee_caps) = callee_caps else {
        return Some(CallContractMismatch {
            callee_display,
            kind: CallContractMismatchKind::UnknownCallee,
            callee_caps: None,
            missing_caps: BTreeSet::new(),
        });
    };
    if callee_caps.rvs_is_empty() || CapabilityPolicy::rvs_can_call(caps, callee_caps) {
        return None;
    }
    Some(CallContractMismatch {
        callee_display,
        kind: CallContractMismatchKind::MissingCapabilities,
        callee_caps: Some(callee_caps.clone()),
        missing_caps: CapabilityPolicy::rvs_missing_for(caps, callee_caps),
    })
}

/// Build a "method@trait_path" → set-of-keys index from callgraph keys.
pub(crate) fn rvs_build_impl_index(graph: &FnGraph) -> HashMap<TraitMethodKey, Vec<DefPath>> {
    let mut idx: HashMap<TraitMethodKey, Vec<DefPath>> = HashMap::new();
    for key in graph.rvs_keys() {
        if let Some(identity) = key.rvs_trait_method_identity() {
            idx.entry(identity.rvs_lookup_key())
                .or_default()
                .push(key.clone());
        }
    }
    idx
}

fn rvs_aggregate_port_methods(graph: &FnGraph) -> BTreeSet<DefPath> {
    let mut methods: BTreeSet<DefPath> = graph
        .rvs_iter()
        .filter(|(_, node)| node.facts.is_port_method)
        .map(|(path, _)| path.clone())
        .collect();
    rvs_include_port_implementations_M(graph, &mut methods);
    methods
}

fn rvs_scoped_port_methods(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> BTreeSet<DefPath> {
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let mut methods: BTreeSet<DefPath> = graph
        .rvs_iter()
        .filter(|(path, node)| {
            scope.rvs_contains_target(path, node.crate_provenance) && node.facts.is_port_method
        })
        .map(|(path, _)| path.clone())
        .collect();
    rvs_include_port_implementations_M(graph, &mut methods);
    methods
}

fn rvs_include_port_implementations_M(graph: &FnGraph, methods: &mut BTreeSet<DefPath>) {
    let operation_paths = methods.clone();
    for path in graph.rvs_keys() {
        let Some(identity) = path.rvs_trait_method_identity() else {
            continue;
        };
        if operation_paths.contains(&identity.rvs_trait_method_path()) {
            methods.insert(path.clone());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalleeCapsSource {
    PortMethod,
    ExactCapsMap,
    BodylessImplWithSignature,
    BodylessDeclaredCaps,
    Inferred,
    DeclaredCaps,
    ImplMajority,
}

const PROPAGATION_TARGET_PRECEDENCE: &[CalleeCapsSource] = &[
    CalleeCapsSource::PortMethod,
    CalleeCapsSource::ExactCapsMap,
    CalleeCapsSource::BodylessImplWithSignature,
    CalleeCapsSource::BodylessDeclaredCaps,
    CalleeCapsSource::Inferred,
    CalleeCapsSource::DeclaredCaps,
    CalleeCapsSource::ImplMajority,
];

const CONTRACT_CHECK_PRECEDENCE: &[CalleeCapsSource] = &[
    CalleeCapsSource::PortMethod,
    CalleeCapsSource::ExactCapsMap,
    CalleeCapsSource::DeclaredCaps,
    CalleeCapsSource::Inferred,
    CalleeCapsSource::ImplMajority,
];

const EXPLANATION_VIEW_PRECEDENCE: &[CalleeCapsSource] = &[
    CalleeCapsSource::Inferred,
    CalleeCapsSource::ExactCapsMap,
    CalleeCapsSource::ImplMajority,
];

#[derive(Debug, Clone, Copy)]
struct CapabilityKnowledgeView<'a> {
    base: &'a capsmap::CapsMap,
    overlay: Option<&'a BTreeMap<DefPath, CapabilityInfo>>,
}

impl<'a> CapabilityKnowledgeView<'a> {
    fn rvs_base(base: &'a capsmap::CapsMap) -> Self {
        Self {
            base,
            overlay: None,
        }
    }

    fn rvs_with_overlay(
        base: &'a capsmap::CapsMap,
        overlay: &'a BTreeMap<DefPath, CapabilityInfo>,
    ) -> Self {
        Self {
            base,
            overlay: Some(overlay),
        }
    }

    fn rvs_lookup_info(self, path: &DefPath) -> Option<&'a CapabilityInfo> {
        self.overlay
            .and_then(|overlay| overlay.get(path))
            .or_else(|| self.base.rvs_lookup_info_def_path(path))
    }

    fn rvs_lookup_caps(self, path: &DefPath) -> Option<&'a CapabilitySet> {
        self.rvs_lookup_info(path).map(CapabilityInfo::rvs_caps)
    }
}

#[derive(Debug)]
pub(crate) struct CalleeCapsResolver<'a> {
    graph: &'a FnGraph,
    knowledge: CapabilityKnowledgeView<'a>,
    inferred: &'a BTreeMap<DefPath, CapabilitySet>,
    impl_index: &'a HashMap<TraitMethodKey, Vec<DefPath>>,
    scoped_port_methods: Option<&'a BTreeSet<DefPath>>,
}

impl<'a> CalleeCapsResolver<'a> {
    pub(crate) fn rvs_new(
        graph: &'a FnGraph,
        caps: &'a capsmap::CapsMap,
        inferred: &'a BTreeMap<DefPath, CapabilitySet>,
        impl_index: &'a HashMap<TraitMethodKey, Vec<DefPath>>,
    ) -> Self {
        Self {
            graph,
            knowledge: CapabilityKnowledgeView::rvs_base(caps),
            inferred,
            impl_index,
            scoped_port_methods: None,
        }
    }

    fn rvs_new_scoped(
        graph: &'a FnGraph,
        caps: &'a capsmap::CapsMap,
        inferred: &'a BTreeMap<DefPath, CapabilitySet>,
        impl_index: &'a HashMap<TraitMethodKey, Vec<DefPath>>,
        scoped_port_methods: &'a BTreeSet<DefPath>,
    ) -> Self {
        Self {
            graph,
            knowledge: CapabilityKnowledgeView::rvs_base(caps),
            inferred,
            impl_index,
            scoped_port_methods: Some(scoped_port_methods),
        }
    }

    fn rvs_new_with_knowledge(
        graph: &'a FnGraph,
        knowledge: CapabilityKnowledgeView<'a>,
        inferred: &'a BTreeMap<DefPath, CapabilitySet>,
        impl_index: &'a HashMap<TraitMethodKey, Vec<DefPath>>,
        scoped_port_methods: &'a BTreeSet<DefPath>,
    ) -> Self {
        Self {
            graph,
            knowledge,
            inferred,
            impl_index,
            scoped_port_methods: Some(scoped_port_methods),
        }
    }

    fn rvs_is_port_method(&self, callee: &DefPath) -> bool {
        self.scoped_port_methods.map_or_else(
            || {
                self.graph
                    .rvs_get(callee.rvs_as_str())
                    .is_some_and(|node| node.facts.is_port_method)
            },
            |ports| ports.contains(callee),
        )
    }

    pub(crate) fn rvs_for_propagation_target(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.rvs_resolve(callee, PROPAGATION_TARGET_PRECEDENCE)
    }

    pub(crate) fn rvs_for_contract_check(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.rvs_resolve(callee, CONTRACT_CHECK_PRECEDENCE)
    }

    pub(crate) fn rvs_incomplete_exact_caps_info(
        &self,
        callee: &DefPath,
    ) -> Option<&CapabilityInfo> {
        if self.rvs_is_port_method(callee) {
            return None;
        }
        self.knowledge
            .rvs_lookup_info(callee)
            .filter(|info| info.rvs_completeness() != CapabilityCompleteness::Complete)
    }

    pub(crate) fn rvs_exact_caps_info(&self, callee: &DefPath) -> Option<&CapabilityInfo> {
        self.knowledge.rvs_lookup_info(callee)
    }

    pub(crate) fn rvs_exact_caps(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.knowledge.rvs_lookup_caps(callee).cloned()
    }

    pub(crate) fn rvs_for_explanation_view(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.rvs_resolve(callee, EXPLANATION_VIEW_PRECEDENCE)
    }

    fn rvs_resolve(
        &self,
        callee: &DefPath,
        precedence: &[CalleeCapsSource],
    ) -> Option<CapabilitySet> {
        precedence
            .iter()
            .find_map(|source| self.rvs_resolve_source(callee, *source))
    }

    fn rvs_resolve_source(
        &self,
        callee: &DefPath,
        source: CalleeCapsSource,
    ) -> Option<CapabilitySet> {
        match source {
            CalleeCapsSource::PortMethod => {
                if !self.rvs_is_port_method(callee) {
                    return None;
                }
                let operation = callee
                    .rvs_trait_method_identity()
                    .map(|identity| identity.rvs_trait_method_path())
                    .unwrap_or_else(|| callee.clone());
                let mut caps = self
                    .inferred
                    .get(&operation)
                    .or_else(|| self.inferred.get(callee))
                    .cloned()
                    .unwrap_or_else(CapabilityPolicy::rvs_port_method_caps);
                let voted = rvs_resolve_impl_majority_caps_with_ports(
                    &operation,
                    self.impl_index,
                    self.inferred,
                    self.graph,
                    self.scoped_port_methods,
                );
                if let Some(voted) = voted {
                    let _ = caps.rvs_extend_filtered_M(&voted, |_| true);
                } else if let Some(declared) = rvs_declared_caps_from_def_path(&operation) {
                    let _ = caps
                        .rvs_extend_filtered_M(&declared, CapabilityPolicy::rvs_is_propagated_cap);
                }
                caps.rvs_insert_M(Capability::P);
                Some(caps)
            }
            CalleeCapsSource::ExactCapsMap => self.knowledge.rvs_lookup_caps(callee).cloned(),
            CalleeCapsSource::BodylessImplWithSignature => {
                self.rvs_bodyless_impl_caps_with_signature(callee)
            }
            CalleeCapsSource::BodylessDeclaredCaps => self
                .graph
                .rvs_get(callee.rvs_as_str())
                .filter(|node| !node.has_body)
                .and_then(|_| rvs_declared_caps_from_def_path(callee)),
            CalleeCapsSource::Inferred => self
                .graph
                .rvs_get(callee.rvs_as_str())
                .is_none_or(|node| {
                    node.has_body
                        || self.rvs_is_port_method(callee)
                        || rvs_declared_caps_from_def_path(callee).is_some()
                })
                .then(|| self.inferred.get(callee).cloned())
                .flatten(),
            CalleeCapsSource::DeclaredCaps => rvs_declared_caps_from_def_path(callee),
            CalleeCapsSource::ImplMajority => {
                if callee.rvs_trait_method_identity().is_some() {
                    None
                } else {
                    rvs_resolve_impl_majority_caps_with_ports(
                        callee,
                        self.impl_index,
                        self.inferred,
                        self.graph,
                        self.scoped_port_methods,
                    )
                }
            }
        }
    }

    fn rvs_bodyless_impl_caps_with_signature(&self, callee: &DefPath) -> Option<CapabilitySet> {
        if callee.rvs_trait_method_identity().is_some()
            || !self
                .graph
                .rvs_get(callee.rvs_as_str())
                .is_some_and(|node| !node.has_body)
        {
            return None;
        }
        let mut caps = rvs_resolve_impl_majority_caps_with_ports(
            callee,
            self.impl_index,
            self.inferred,
            self.graph,
            self.scoped_port_methods,
        )?;
        if let Some(signature_caps) = self.inferred.get(callee) {
            let _ = caps.rvs_extend_filtered_M(signature_caps, |cap| {
                !CapabilityPolicy::rvs_is_propagated_cap(cap)
            });
        }
        Some(caps)
    }
}

/// Infer capabilities from behavioral flags alone (no propagation).
pub(crate) fn rvs_infer_signature_caps(behavior: &FnNode) -> CapabilitySet {
    CapabilityPolicy::rvs_signature_caps(behavior.facts)
}

/// Format an error message for unknown callees.
pub(crate) fn rvs_format_unknown_callees(
    unknown: &BTreeMap<DefPath, BTreeSet<DefPath>>,
    header: &str,
) -> String {
    let mut normalized: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (callee, callers) in unknown {
        normalized
            .entry(callee.rvs_user_path().into_owned())
            .or_default()
            .extend(callers.iter().map(ToString::to_string));
    }
    let mut msg = String::from(header);
    for (callee, callers) in normalized {
        msg.push_str(&format!("  {callee}=\n"));
        for caller in callers.iter().take(3) {
            msg.push_str(&format!("    called by: {caller}\n"));
        }
        if callers.len() > 3 {
            msg.push_str(&format!("    ... and {} more\n", callers.len() - 3));
        }
    }
    msg
}

/// Generate trait-method aliases (e.g. `std::io::Read::read`) from impl-method
/// keys using at-least-half capability aggregation across impls.
#[cfg(test)]
pub(crate) fn rvs_generate_trait_aliases(
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    graph: &FnGraph,
) -> BTreeMap<DefPath, CapabilitySet> {
    let mut aliases = BTreeMap::new();
    let mut seen = HashSet::new();
    for key in inferred.keys() {
        if let Some(identity) = key.rvs_trait_method_identity() {
            let alias = identity.rvs_trait_method_path();
            if seen.insert(alias.clone())
                && let Some(voted) =
                    rvs_resolve_impl_majority_caps(&alias, impl_index, inferred, graph)
            {
                aliases.insert(alias, voted);
            }
        }
    }
    aliases
}

pub(crate) fn rvs_generate_trait_alias_infos(
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    graph: &FnGraph,
    incomplete_paths: &BTreeSet<DefPath>,
) -> BTreeMap<DefPath, CapabilityInfo> {
    let mut aliases = BTreeMap::new();
    let mut seen = HashSet::new();
    for key in inferred.keys() {
        let Some(identity) = key.rvs_trait_method_identity() else {
            continue;
        };
        let alias = identity.rvs_trait_method_path();
        if !seen.insert(alias.clone()) {
            continue;
        }
        let Some(vote) =
            rvs_resolve_impl_capability_vote(&alias, impl_index, inferred, graph, incomplete_paths)
        else {
            continue;
        };
        let lower_bound = inferred.get(&alias).unwrap_or(&vote.selected_caps);
        let info = vote
            .rvs_capability_info_with_lower_bound(lower_bound, incomplete_paths.contains(&alias));
        aliases.insert(alias, info);
    }
    aliases
}

/// Convert a `CapabilitySet` to its uppercase letter string.
#[cfg(test)]
pub(crate) fn rvs_caps_to_string(caps: &CapabilitySet) -> String {
    caps.rvs_letters()
}

#[cfg(test)]
pub(crate) fn rvs_infer_caps(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
) -> BTreeMap<DefPath, CapabilitySet> {
    let impl_index = rvs_build_impl_index(graph);
    rvs_infer_caps_with_index(graph, seed, &impl_index)
}

#[cfg(test)]
pub(crate) fn rvs_infer_caps_with_index(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> BTreeMap<DefPath, CapabilitySet> {
    let port_methods = rvs_aggregate_port_methods(graph);
    rvs_infer_caps_with_index_and_ports(graph, seed, impl_index, &port_methods)
}

pub(crate) fn rvs_infer_caps_with_index_overlay_and_dependents(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    overlay: &BTreeMap<DefPath, CapabilityInfo>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    dependents: &InferenceDependents,
) -> BTreeMap<DefPath, CapabilitySet> {
    let port_methods = rvs_aggregate_port_methods(graph);
    rvs_infer_caps_with_knowledge_and_ports(
        graph,
        CapabilityKnowledgeView::rvs_with_overlay(seed, overlay),
        impl_index,
        &port_methods,
        dependents,
    )
}

#[cfg(test)]
fn rvs_infer_caps_with_index_and_ports(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    port_methods: &BTreeSet<DefPath>,
) -> BTreeMap<DefPath, CapabilitySet> {
    let dependents = rvs_build_inference_dependents(graph, impl_index);
    rvs_infer_caps_with_knowledge_and_ports(
        graph,
        CapabilityKnowledgeView::rvs_base(seed),
        impl_index,
        port_methods,
        &dependents,
    )
}

fn rvs_infer_caps_with_knowledge_and_ports(
    graph: &FnGraph,
    knowledge: CapabilityKnowledgeView<'_>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    port_methods: &BTreeSet<DefPath>,
    dependents: &InferenceDependents,
) -> BTreeMap<DefPath, CapabilitySet> {
    let mut inferred = rvs_initial_caps_with_knowledge(graph, knowledge, port_methods);
    for (_, behavior) in graph.rvs_iter() {
        for callee in behavior.calls.keys() {
            if !inferred.contains_key(&callee.def_path)
                && let Some(caps) = knowledge.rvs_lookup_caps(&callee.def_path)
            {
                inferred.insert(callee.def_path.clone(), caps.clone());
            } else if !inferred.contains_key(&callee.def_path)
                && let Some(caps) = rvs_declared_caps_from_def_path(&callee.def_path)
            {
                inferred.insert(callee.def_path.clone(), caps);
            }
        }
    }

    let mut pending: VecDeque<DefPath> = inferred
        .iter()
        .filter(|(_, caps)| !rvs_propagated_caps(caps).rvs_is_empty())
        .map(|(path, _)| path.clone())
        .collect();
    let mut queued: HashSet<DefPath> = pending.iter().cloned().collect();
    let mut published: HashMap<DefPath, CapabilitySet> = HashMap::new();
    while let Some(func) = pending.pop_front() {
        queued.remove(&func);
        let voted_caps = (knowledge.rvs_lookup_caps(&func).is_none())
            .then(|| {
                rvs_resolve_impl_majority_caps_with_ports(
                    &func,
                    impl_index,
                    &inferred,
                    graph,
                    Some(port_methods),
                )
            })
            .flatten();
        let resolver = CalleeCapsResolver::rvs_new_with_knowledge(
            graph,
            knowledge,
            &inferred,
            impl_index,
            port_methods,
        );
        let is_port_impl = port_methods.contains(&func)
            && graph.rvs_get(func.rvs_as_str()).is_some_and(|node| {
                node.is_trait_impl || func.rvs_trait_method_identity().is_some()
            });
        let Some(mut effective_caps) = is_port_impl
            .then(|| inferred.get(&func).cloned())
            .flatten()
            .or_else(|| resolver.rvs_for_propagation_target(&func))
            .or_else(|| voted_caps.clone())
        else {
            continue;
        };
        if let Some(voted_caps) = voted_caps {
            let _ = effective_caps.rvs_extend_filtered_M(&voted_caps, |_| true);
            let inferred_caps = inferred
                .entry(func.clone())
                .or_insert_with(CapabilitySet::rvs_new);
            let _ = inferred_caps.rvs_extend_filtered_M(&effective_caps, |_| true);
        }
        let propagated_caps = rvs_propagated_caps(&effective_caps);
        if propagated_caps.rvs_is_empty() {
            continue;
        }
        let published_caps = published
            .entry(func.clone())
            .or_insert_with(CapabilitySet::rvs_new);
        if !published_caps.rvs_extend_filtered_M(&propagated_caps, |_| true) {
            continue;
        }
        if let Some(callers) = dependents.callers.get(&func) {
            for caller in callers {
                if graph.rvs_get(caller.rvs_as_str()).is_none() {
                    continue;
                }
                let caller_is_port_operation = port_methods.contains(caller)
                    && graph.rvs_get(caller.rvs_as_str()).is_some_and(|node| {
                        !node.is_trait_impl && caller.rvs_trait_method_identity().is_none()
                    });
                if knowledge.rvs_lookup_caps(caller).is_some() || caller_is_port_operation {
                    continue;
                }
                let call_edge_caps = CapabilityPolicy::rvs_call_edge_caps(&effective_caps);
                let caller_caps = inferred
                    .entry(caller.clone())
                    .or_insert_with(CapabilitySet::rvs_new);
                if caller_caps.rvs_extend_filtered_M(&call_edge_caps, |_| true)
                    && queued.insert(caller.clone())
                {
                    pending.push_back(caller.clone());
                }
            }
        }
        if let Some(trait_methods) = dependents.trait_methods.get(&func) {
            for trait_method in trait_methods {
                if queued.insert(trait_method.clone()) {
                    pending.push_back(trait_method.clone());
                }
            }
        }
    }
    let bodyless_paths: Vec<DefPath> = graph
        .rvs_iter()
        .filter(|(_, behavior)| !behavior.has_body)
        .map(|(path, _)| path.clone())
        .collect();
    for func in bodyless_paths {
        if knowledge.rvs_lookup_caps(&func).is_some() {
            continue;
        }
        if let Some(caps) = CalleeCapsResolver::rvs_new_with_knowledge(
            graph,
            knowledge,
            &inferred,
            impl_index,
            port_methods,
        )
        .rvs_for_propagation_target(&func)
        {
            inferred.insert(func, caps);
        }
    }
    inferred
}

pub(crate) struct InferenceDependents {
    callers: HashMap<DefPath, BTreeSet<DefPath>>,
    trait_methods: HashMap<DefPath, BTreeSet<DefPath>>,
}

pub(crate) fn rvs_build_inference_dependents(
    graph: &FnGraph,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> InferenceDependents {
    let mut callers: HashMap<DefPath, BTreeSet<DefPath>> = HashMap::new();
    for (caller, behavior) in graph.rvs_iter() {
        for callee in behavior.calls.keys() {
            callers
                .entry(callee.def_path.clone())
                .or_default()
                .insert(caller.clone());
        }
    }
    let mut trait_methods: HashMap<DefPath, BTreeSet<DefPath>> = HashMap::new();
    for implementations in impl_index.values() {
        let Some(trait_method) = implementations
            .iter()
            .find_map(|implementation| implementation.rvs_trait_method_identity())
            .map(|identity| identity.rvs_trait_method_path())
        else {
            continue;
        };
        for implementation in implementations {
            trait_methods
                .entry(implementation.clone())
                .or_default()
                .insert(trait_method.clone());
        }
    }
    InferenceDependents {
        callers,
        trait_methods,
    }
}

#[cfg(test)]
pub(crate) fn rvs_initial_caps(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
) -> BTreeMap<DefPath, CapabilitySet> {
    let port_methods = rvs_aggregate_port_methods(graph);
    rvs_initial_caps_with_ports(graph, seed, &port_methods)
}

#[cfg(test)]
fn rvs_initial_caps_with_ports(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    port_methods: &BTreeSet<DefPath>,
) -> BTreeMap<DefPath, CapabilitySet> {
    rvs_initial_caps_with_knowledge(graph, CapabilityKnowledgeView::rvs_base(seed), port_methods)
}

fn rvs_initial_caps_with_knowledge(
    graph: &FnGraph,
    knowledge: CapabilityKnowledgeView<'_>,
    port_methods: &BTreeSet<DefPath>,
) -> BTreeMap<DefPath, CapabilitySet> {
    graph
        .rvs_iter()
        .map(|(func, behavior)| {
            let caps = if port_methods.contains(func) {
                let mut facts = behavior.facts;
                facts.is_port_method = true;
                if behavior.is_trait_impl || func.rvs_trait_method_identity().is_some() {
                    CapabilityPolicy::rvs_signature_caps(facts)
                } else {
                    CapabilityPolicy::rvs_port_operation_signature_caps(facts)
                }
            } else if let Some(caps) = knowledge.rvs_lookup_caps(func) {
                caps.clone()
            } else {
                rvs_infer_signature_caps(behavior)
            };
            (func.clone(), caps)
        })
        .collect()
}

pub(crate) fn rvs_scope_port_methods_M(
    graph: &mut FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    for (def_path, node) in graph.rvs_iter_mut_M() {
        let identity = FunctionIdentity {
            crate_id: node.crate_id,
            def_path: def_path.clone(),
        };
        if !scope.rvs_contains_identity(&identity) {
            node.facts.is_port_method = false;
        }
    }
    debug_assert!(graph.rvs_iter().all(|(def_path, node)| {
        let identity = FunctionIdentity {
            crate_id: node.crate_id,
            def_path: def_path.clone(),
        };
        scope.rvs_contains_identity(&identity) || !node.facts.is_port_method
    }));
}

fn rvs_declared_caps_from_def_path(def_path: &DefPath) -> Option<CapabilitySet> {
    ParsedFunctionName::rvs_parse(def_path.rvs_as_str()).rvs_declared_caps()
}

fn rvs_expected_contract_name(name: &FnName, caps: &CapabilitySet) -> FnName {
    let caps_str = caps.rvs_letters();
    let base_name = rvs_contract_base_name(name.rvs_as_str(), &caps_str);
    if caps_str.is_empty() {
        FnName::rvs_new(format!("rvs_{base_name}"))
    } else {
        FnName::rvs_new(format!("rvs_{base_name}_{caps_str}"))
    }
}

pub(crate) fn rvs_contract_diff_for_expected_caps(
    def_path: &DefPath,
    expected_public_caps: CapabilitySet,
    incomplete: bool,
) -> FnContractDiff {
    let actual_name = def_path.rvs_fn_name();
    let declared_public_caps = rvs_parse_function(actual_name.rvs_as_str()).map(|(_, caps)| caps);
    let mut naming_caps = expected_public_caps.clone();
    if incomplete && let Some(declared_caps) = &declared_public_caps {
        let _ = naming_caps.rvs_extend_filtered_M(declared_caps, |capability| {
            CapabilityPolicy::rvs_is_propagated_cap(capability)
        });
    }
    let expected_name = rvs_expected_contract_name(&actual_name, &naming_caps);
    FnContractDiff {
        def_path: def_path.clone(),
        actual_name,
        expected_name,
        declared_public_caps,
        expected_public_caps,
    }
}

fn rvs_contract_base_name<'a>(name: &'a str, expected_caps: &str) -> &'a str {
    if let Some((base, _)) = rvs_parse_function(name) {
        return base;
    }
    if let Some((base, suffix)) = name.rsplit_once('_')
        && !suffix.is_empty()
        && !expected_caps.is_empty()
        && suffix
            .chars()
            .all(|c| c.is_ascii_uppercase() && Capability::rvs_from_char(c).is_some())
    {
        return base;
    }
    name
}

#[cfg(test)]
fn rvs_collect_contract_diffs(
    graph: &FnGraph,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    rvs_collect_contract_diffs_with_incomplete(graph, inferred, local_crate_names, &BTreeSet::new())
}

fn rvs_collect_contract_diffs_with_incomplete(
    graph: &FnGraph,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    local_crate_names: &BTreeSet<CrateName>,
    incomplete_paths: &BTreeSet<DefPath>,
) -> Vec<FnContractDiff> {
    let scope = LocalScope::rvs_for_graph(local_crate_names, graph);
    let mut diffs = Vec::new();
    for (def_path, node) in graph.rvs_iter() {
        if !FunctionClassification::rvs_new(&scope, def_path, node).rvs_is_contract_enforced() {
            continue;
        }
        let expected_public_caps = inferred
            .get(def_path)
            .expect("never: prepared inference covers every graph node");
        diffs.push(rvs_contract_diff_for_expected_caps(
            def_path,
            expected_public_caps.clone(),
            incomplete_paths.contains(def_path),
        ));
    }
    diffs
}

pub(crate) fn rvs_incomplete_inference_paths_overlay_and_dependents(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    overlay: &BTreeMap<DefPath, CapabilityInfo>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    dependents: &InferenceDependents,
) -> BTreeSet<DefPath> {
    let port_methods = rvs_aggregate_port_methods(graph);
    rvs_incomplete_inference_paths_with_knowledge(
        graph,
        CapabilityKnowledgeView::rvs_with_overlay(seed, overlay),
        inferred,
        impl_index,
        &port_methods,
        dependents,
    )
}

#[cfg(test)]
fn rvs_incomplete_inference_paths_with_ports(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    port_methods: &BTreeSet<DefPath>,
) -> BTreeSet<DefPath> {
    let dependents = rvs_build_inference_dependents(graph, impl_index);
    rvs_incomplete_inference_paths_with_knowledge(
        graph,
        CapabilityKnowledgeView::rvs_base(seed),
        inferred,
        impl_index,
        port_methods,
        &dependents,
    )
}

fn rvs_incomplete_inference_paths_with_knowledge(
    graph: &FnGraph,
    knowledge: CapabilityKnowledgeView<'_>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    port_methods: &BTreeSet<DefPath>,
    dependents: &InferenceDependents,
) -> BTreeSet<DefPath> {
    let resolver = CalleeCapsResolver::rvs_new_with_knowledge(
        graph,
        knowledge,
        inferred,
        impl_index,
        port_methods,
    );
    let mut incomplete: BTreeSet<DefPath> = graph
        .rvs_iter()
        .filter(|(path, node)| {
            if rvs_is_inference_taint_barrier(path, knowledge) {
                return false;
            }
            if !node.complete || rvs_has_incomplete_capsmap_knowledge(path, knowledge, port_methods)
            {
                return true;
            }
            let has_declared_caps = ParsedFunctionName::rvs_parse(path.rvs_as_str())
                .rvs_declared_caps()
                .is_some();
            let has_exact = knowledge.rvs_lookup_info(path).is_some();
            !node.has_body
                && !port_methods.contains(*path)
                && !has_exact
                && !has_declared_caps
                && resolver.rvs_for_contract_check(path).is_none()
        })
        .map(|(path, _)| path.clone())
        .collect();

    for (callee, callers) in &dependents.callers {
        if resolver.rvs_for_contract_check(callee).is_some()
            && !rvs_has_incomplete_capsmap_knowledge(callee, knowledge, port_methods)
        {
            continue;
        }
        incomplete.extend(
            callers
                .iter()
                .filter(|caller| !rvs_is_inference_taint_barrier(caller, knowledge))
                .cloned(),
        );
    }

    let mut pending: VecDeque<DefPath> = incomplete.iter().cloned().collect();
    while let Some(path) = pending.pop_front() {
        if !inferred
            .get(&path)
            .is_some_and(|caps| caps.rvs_contains(Capability::P))
            && let Some(callers) = dependents.callers.get(&path)
        {
            for caller in callers {
                if rvs_is_inference_taint_barrier(caller, knowledge) {
                    continue;
                }
                if incomplete.insert(caller.clone()) {
                    pending.push_back(caller.clone());
                }
            }
        }
        let Some(trait_methods) = dependents.trait_methods.get(&path) else {
            continue;
        };
        for trait_method in trait_methods {
            if rvs_is_inference_taint_barrier(trait_method, knowledge) {
                continue;
            }
            if incomplete.insert(trait_method.clone()) {
                pending.push_back(trait_method.clone());
            }
        }
    }
    incomplete
}

fn rvs_is_inference_taint_barrier(path: &DefPath, knowledge: CapabilityKnowledgeView<'_>) -> bool {
    knowledge
        .rvs_lookup_info(path)
        .is_some_and(|info| info.rvs_completeness() == CapabilityCompleteness::Complete)
}

fn rvs_has_incomplete_capsmap_knowledge(
    path: &DefPath,
    knowledge: CapabilityKnowledgeView<'_>,
    port_methods: &BTreeSet<DefPath>,
) -> bool {
    !port_methods.contains(path)
        && knowledge
            .rvs_lookup_info(path)
            .is_some_and(|info| info.rvs_completeness() != CapabilityCompleteness::Complete)
}

#[cfg(test)]
pub(crate) fn rvs_collect_local_contract_diffs_M(
    graph: &mut FnGraph,
    seed: &capsmap::CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    PreparedLocalAnalysis::rvs_prepare_M(graph, seed, local_crate_names).diffs
}

pub(crate) fn rvs_summarize_contract_mismatch_items(
    items: &[FnContractMismatch],
) -> BTreeMap<FnContractMismatchKind, usize> {
    let mut counts = BTreeMap::new();
    for mismatch in items {
        *counts.entry(mismatch.kind).or_default() += 1;
    }
    counts
}

pub(crate) fn rvs_collect_contract_mismatch_items(
    diffs: &[FnContractDiff],
) -> Vec<FnContractMismatch> {
    let mut items = Vec::new();
    for diff in diffs {
        for kind in diff.rvs_mismatch_kinds() {
            items.push(FnContractMismatch {
                def_path: diff.def_path.clone(),
                actual_name: diff.actual_name.clone(),
                kind,
            });
        }
    }
    items.sort();
    items
}

#[cfg(test)]
pub(crate) fn rvs_collect_single_local_contract_diff_M(
    def_path: DefPath,
    node: FnNode,
    local_crate_names: &BTreeSet<CrateName>,
) -> FnContractDiff {
    let mut graph = FnGraph::rvs_new();
    graph.rvs_insert_M(def_path, node);
    rvs_collect_local_contract_diffs_M(&mut graph, &capsmap::CapsMap::rvs_new(), local_crate_names)
        .into_iter()
        .next()
        .expect("never: single local contract diff should always exist")
}

#[cfg(test)]
pub(crate) fn rvs_collect_signature_contract_diff_from_facts_M(
    def_path: DefPath,
    facts: crate::capability::CapabilityFacts,
    local_crate_names: &BTreeSet<CrateName>,
) -> FnContractDiff {
    rvs_collect_single_local_contract_diff_M(
        def_path,
        FnNode {
            facts,
            sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                "src/lib.rs".into(),
                1,
                2,
            )]),
            ..FnNode::default()
        },
        local_crate_names,
    )
}

fn rvs_propagated_caps(caps: &CapabilitySet) -> CapabilitySet {
    let mut propagated = CapabilitySet::rvs_new();
    let _ = propagated.rvs_extend_filtered_M(caps, CapabilityPolicy::rvs_is_propagated_cap);
    propagated
}

pub(crate) fn rvs_resolve_impl_capability_vote(
    callee: &DefPath,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
    incomplete_paths: &BTreeSet<DefPath>,
) -> Option<TraitCapabilityVote> {
    rvs_resolve_impl_capability_vote_with_ports(
        callee,
        impl_index,
        inferred,
        graph,
        incomplete_paths,
        None,
    )
}

fn rvs_resolve_impl_capability_vote_with_ports(
    callee: &DefPath,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
    incomplete_paths: &BTreeSet<DefPath>,
    port_methods: Option<&BTreeSet<DefPath>>,
) -> Option<TraitCapabilityVote> {
    let lookup_key = TraitMethodKey::rvs_from_trait_method(callee)?;
    let impl_keys = impl_index.get(&lookup_key)?;
    let is_port = port_methods.is_some_and(|ports| ports.contains(callee))
        || impl_keys.iter().any(|key| {
            port_methods.map_or_else(
                || {
                    graph
                        .rvs_get(key.rvs_as_str())
                        .is_some_and(|behavior| behavior.facts.is_port_method)
                },
                |ports| ports.contains(key),
            )
        });
    let mut implementations = Vec::new();
    for key in impl_keys {
        if let Some(caps) = inferred.get(key) {
            let propagated_caps = rvs_propagated_caps(caps);
            implementations.push(TraitVoteImplementation {
                path: key.clone(),
                propagated_caps,
                incomplete: incomplete_paths.contains(key),
            });
        }
    }
    implementations.sort_by(|left, right| left.path.cmp(&right.path));
    let (voted_caps, threshold, counts) = CapabilityPolicy::rvs_at_least_half_vote(
        implementations
            .iter()
            .map(|implementation| &implementation.propagated_caps),
    )?;
    let mut selected_caps = voted_caps;
    if is_port {
        selected_caps.rvs_insert_M(Capability::P);
    }
    Some(TraitCapabilityVote {
        trait_method: callee.clone(),
        selected_caps,
        implementations,
        threshold,
        counts,
        is_port,
    })
}

/// Resolve a trait method callee by taking an at-least-half vote across all
/// impl methods for each propagated capability.
#[cfg(test)]
pub(crate) fn rvs_resolve_impl_majority_caps(
    callee: &DefPath,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
) -> Option<CapabilitySet> {
    rvs_resolve_impl_majority_caps_with_ports(callee, impl_index, inferred, graph, None)
}

fn rvs_resolve_impl_majority_caps_with_ports(
    callee: &DefPath,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
    port_methods: Option<&BTreeSet<DefPath>>,
) -> Option<CapabilitySet> {
    rvs_resolve_impl_capability_vote_with_ports(
        callee,
        impl_index,
        inferred,
        graph,
        &BTreeSet::new(),
        port_methods,
    )
    .map(|vote| vote.selected_caps)
}

fn rvs_collect_trait_votes_with_ports(
    graph: &FnGraph,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    incomplete_paths: &BTreeSet<DefPath>,
    port_methods: &BTreeSet<DefPath>,
) -> BTreeMap<DefPath, TraitCapabilityVote> {
    let mut votes = BTreeMap::new();
    for implementations in impl_index.values() {
        let Some(trait_method) = implementations
            .iter()
            .find_map(|implementation| implementation.rvs_trait_method_identity())
            .map(|identity| identity.rvs_trait_method_path())
        else {
            continue;
        };
        if votes.contains_key(&trait_method) {
            continue;
        }
        if let Some(vote) = rvs_resolve_impl_capability_vote_with_ports(
            &trait_method,
            impl_index,
            inferred,
            graph,
            incomplete_paths,
            Some(port_methods),
        ) {
            votes.insert(trait_method, vote);
        }
    }
    votes
}

#[cfg(test)]
pub(crate) fn rvs_format_capsmap<K>(caps: &BTreeMap<K, CapabilitySet>) -> String
where
    K: AsRef<str> + Ord,
{
    let mut map = capsmap::CapsMap::rvs_new();
    map.rvs_extend_info_entries_M(caps.iter().map(|(name, caps)| {
        (
            CapsMapKey::rvs_new(name.as_ref().to_string()),
            crate::capability::CapabilityInfo::rvs_inferred(caps.clone()),
        )
    }));
    map.rvs_render_v2()
}

#[cfg(test)]
pub(crate) fn rvs_format_def_path_capsmap(caps: &BTreeMap<DefPath, CapabilitySet>) -> String {
    let mut normalized: BTreeMap<CapsMapKey, CapabilitySet> = BTreeMap::new();
    for (path, path_caps) in caps {
        let user_path = path.rvs_user_path();
        let combined = normalized
            .entry(CapsMapKey::rvs_new(user_path.into_owned()))
            .or_insert_with(CapabilitySet::rvs_new);
        let _ = combined.rvs_extend_filtered_M(path_caps, |_| true);
    }
    rvs_format_capsmap(&normalized)
}

pub(crate) fn rvs_format_def_path_capability_info(
    infos: &BTreeMap<DefPath, CapabilityInfo>,
) -> String {
    let mut normalized: BTreeMap<CapsMapKey, CapabilityInfo> = BTreeMap::new();
    for (path, info) in infos {
        let key = CapsMapKey::rvs_new(path.rvs_user_path().into_owned());
        if let Some(existing) = normalized.get_mut(&key) {
            let mut combined = existing.rvs_caps().clone();
            let _ = combined.rvs_extend_filtered_M(info.rvs_caps(), |_| true);
            *existing = rvs_combine_capability_info(existing, info, combined);
        } else {
            normalized.insert(key, info.clone());
        }
    }
    let mut map = capsmap::CapsMap::rvs_new();
    map.rvs_extend_info_entries_M(normalized);
    map.rvs_render_v2()
}

fn rvs_combine_capability_info(
    left: &CapabilityInfo,
    right: &CapabilityInfo,
    caps: CapabilitySet,
) -> CapabilityInfo {
    if left.rvs_caps() == &caps
        && right.rvs_caps() == &caps
        && left.rvs_basis() == right.rvs_basis()
        && left.rvs_completeness() == right.rvs_completeness()
    {
        return left.clone();
    }

    match rvs_least_complete_knowledge(left.rvs_completeness(), right.rvs_completeness()) {
        CapabilityCompleteness::Complete => CapabilityInfo::rvs_inferred(caps),
        CapabilityCompleteness::Incomplete => CapabilityInfo::rvs_new(
            caps,
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Incomplete,
        ),
        CapabilityCompleteness::Unknown => CapabilityInfo::rvs_new(
            caps,
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Unknown,
        ),
    }
}

fn rvs_least_complete_knowledge(
    left: CapabilityCompleteness,
    right: CapabilityCompleteness,
) -> CapabilityCompleteness {
    match (left, right) {
        (CapabilityCompleteness::Unknown, _) | (_, CapabilityCompleteness::Unknown) => {
            CapabilityCompleteness::Unknown
        }
        (CapabilityCompleteness::Incomplete, _) | (_, CapabilityCompleteness::Incomplete) => {
            CapabilityCompleteness::Incomplete
        }
        (CapabilityCompleteness::Complete, CapabilityCompleteness::Complete) => {
            CapabilityCompleteness::Complete
        }
    }
}

#[cfg(test)]
pub(crate) fn rvs_collect_direct_external_deps(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> (
    BTreeMap<DefPath, CapabilityInfo>,
    BTreeMap<DefPath, BTreeSet<DefPath>>,
) {
    let scoped_port_methods = rvs_scoped_port_methods(graph, local_crate_names);
    let incomplete_paths = rvs_incomplete_inference_paths_with_ports(
        graph,
        seed,
        inferred,
        impl_index,
        &scoped_port_methods,
    );
    let prepared = PreparedInference {
        inferred: inferred.clone(),
        impl_index: impl_index.clone(),
        scoped_port_methods: scoped_port_methods.clone(),
        synthetic_paths: BTreeSet::new(),
        trait_votes: rvs_collect_trait_votes_with_ports(
            graph,
            impl_index,
            inferred,
            &incomplete_paths,
            &scoped_port_methods,
        ),
        incomplete_paths,
    };
    prepared.rvs_collect_direct_external_deps(graph, local_crate_names, seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::CallEdgeType;
    use crate::capability::CapabilityFacts;
    use crate::test_support::{rvs_make_capsmap, rvs_snapshot_BIS};

    /// Helper: build a default `FnNode` with all flags false and no calls.
    fn rvs_make_behavior() -> FnNode {
        FnNode {
            crate_id: 1,
            crate_provenance: crate::artifacts::CrateProvenance::PrimaryPackage,
            is_production: true,
            sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                "src/lib.rs".into(),
                1,
                2,
            )]),
            ..FnNode::default()
        }
    }

    #[test]
    fn test_20260720_prepare_inference_scales_on_reverse_ordered_chain() {
        const NODE_COUNT: usize = 2_000;
        let mut graph = FnGraph::rvs_new();
        for index in 0..NODE_COUNT {
            let mut node = rvs_make_behavior();
            if index + 1 < NODE_COUNT {
                node.calls.insert(
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(format!("chain::rvs_node_{:04}", index + 1)),
                    },
                    CallEdgeType::Strong,
                );
            } else {
                node.facts.has_static_ref = true;
            }
            graph.rvs_insert_M(DefPath::from(format!("chain::rvs_node_{index:04}")), node);
        }
        let started = std::time::Instant::now();

        let inference = PreparedInference::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("chain")]),
        );

        let elapsed = started.elapsed();
        let first_caps = inference
            .rvs_inferred()
            .get(&DefPath::from("chain::rvs_node_0000"))
            .map(rvs_caps_to_string)
            .unwrap_or_default();
        let last_caps = inference
            .rvs_inferred()
            .get(&DefPath::from("chain::rvs_node_1999"))
            .map(rvs_caps_to_string)
            .unwrap_or_default();
        let output = format!(
            "nodes={}\nfirst_caps={first_caps}\nlast_caps={last_caps}\nincomplete={}\n",
            inference.rvs_inferred().len(),
            inference.rvs_incomplete_paths().len(),
        );
        rvs_snapshot_BIS(
            "test_20260720_prepare_inference_scales_on_reverse_ordered_chain",
            &output,
        );

        assert_eq!(first_caps, "S");
        assert_eq!(last_caps, "S");
        assert!(inference.rvs_incomplete_paths().is_empty());
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "reverse-ordered propagation took {elapsed:?}"
        );
    }

    #[test]
    fn test_20260720_incomplete_propagation_scales_on_reverse_ordered_chain() {
        const NODE_COUNT: usize = 2_000;
        let mut graph = FnGraph::rvs_new();
        for index in 0..NODE_COUNT {
            let mut node = rvs_make_behavior();
            node.calls.insert(
                if index + 1 < NODE_COUNT {
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(format!("chain::rvs_node_{:04}", index + 1)),
                    }
                } else {
                    FunctionIdentity {
                        crate_id: 2,
                        def_path: DefPath::from("opaque::missing"),
                    }
                },
                CallEdgeType::Strong,
            );
            graph.rvs_insert_M(DefPath::from(format!("chain::rvs_node_{index:04}")), node);
        }
        let started = std::time::Instant::now();

        let inference = PreparedInference::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("chain")]),
        );

        let elapsed = started.elapsed();
        let first_incomplete = inference
            .rvs_incomplete_paths()
            .contains(&DefPath::from("chain::rvs_node_0000"));
        let output = format!(
            "nodes={}\nincomplete={}\nfirst_incomplete={first_incomplete}\n",
            inference.rvs_inferred().len(),
            inference.rvs_incomplete_paths().len(),
        );
        rvs_snapshot_BIS(
            "test_20260720_incomplete_propagation_scales_on_reverse_ordered_chain",
            &output,
        );

        assert!(first_incomplete);
        assert_eq!(inference.rvs_incomplete_paths().len(), NODE_COUNT);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "reverse-ordered incomplete propagation took {elapsed:?}"
        );
    }

    #[test]
    fn test_20260711_prepare_local_analysis_builds_shared_derivatives() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let mut impl_method = rvs_make_behavior();
        impl_method.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Service::rvs_load@demo::Repository"),
            impl_method,
        );
        let mut seed = capsmap::CapsMap::rvs_new();
        let mut external_caps = CapabilitySet::rvs_new();
        external_caps.rvs_insert_M(Capability::B);
        external_caps.rvs_insert_M(Capability::I);
        seed.rvs_insert_M(CapsMapKey::from("dep::read"), external_caps);
        let local = BTreeSet::from([CrateName::from("demo")]);

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut graph, &seed, &local);
        let run_caps = analysis
            .rvs_inferred()
            .get(&DefPath::from("demo::rvs_run"))
            .map(rvs_caps_to_string)
            .unwrap_or_default();
        let output = format!(
            "diffs={}\ninferred={}\nimpl_keys={}\nsynthetic={}\ngraph_nodes={}\nrun_caps={run_caps}\n",
            analysis.diffs.len(),
            analysis.rvs_inferred().len(),
            analysis.inference.rvs_impl_index().len(),
            analysis.rvs_synthetic_paths().len(),
            graph.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260711_prepare_local_analysis_builds_shared_derivatives",
            &output,
        );

        assert_eq!(run_caps, "BI");
        assert_eq!(analysis.diffs.len(), 1);
        assert_eq!(analysis.inference.rvs_impl_index().len(), 1);
        assert_eq!(
            analysis.rvs_synthetic_paths(),
            &BTreeSet::from([DefPath::from("dep::read")])
        );
        assert_eq!(graph.rvs_len(), 2);
    }

    #[test]
    fn test_20260715_unknown_callees_suppress_provisional_name_mismatches() {
        let mut graph = FnGraph::rvs_new();
        let mut inner = rvs_make_behavior();
        inner.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_inner_BIS"), inner);
        let mut outer = rvs_make_behavior();
        outer.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_inner_BIS"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_outer_BIS"), outer);
        let local = BTreeSet::from([CrateName::from("demo")]);

        let diffs =
            rvs_collect_local_contract_diffs_M(&mut graph, &capsmap::CapsMap::rvs_new(), &local);
        let output = diffs
            .iter()
            .map(|diff| {
                format!(
                    "{}: name_mismatch={} kinds={}",
                    diff.def_path,
                    diff.rvs_has_name_mismatch(),
                    diff.rvs_mismatch_kinds()
                        .iter()
                        .map(|kind| kind.rvs_as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS(
            "test_20260715_unknown_callees_suppress_provisional_name_mismatches",
            &output,
        );

        assert!(diffs.iter().all(|diff| !diff.rvs_has_name_mismatch()));
    }

    #[test]
    fn test_20260715_incomplete_inference_only_preserves_propagated_declared_caps() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_AMU"), behavior);

        let diff = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        )
        .into_iter()
        .next()
        .expect("never: local function produces a contract diff");
        let output = format!(
            "expected={}\nname_mismatch={}\nkinds={}\n",
            diff.expected_name,
            diff.rvs_has_name_mismatch(),
            diff.rvs_mismatch_kinds()
                .iter()
                .map(|kind| kind.rvs_as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        rvs_snapshot_BIS(
            "test_20260715_incomplete_inference_only_preserves_propagated_declared_caps",
            &output,
        );

        assert_eq!(diff.expected_name.rvs_as_str(), "rvs_run");
        assert!(diff.rvs_has_name_mismatch());
    }

    #[test]
    fn test_20260715_incomplete_inference_stops_at_authoritative_boundaries() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::ApiClient::rvs_fetch_P"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_call_PS"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let mut impl_method = rvs_make_behavior();
        impl_method.is_trait_impl = true;
        impl_method.facts.is_port_method = true;
        impl_method.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::DiskClient::rvs_fetch_P@demo::ApiClient"),
            impl_method,
        );

        let mut seeded = rvs_make_behavior();
        seeded.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_seeded_BIS"), seeded);
        let seed = rvs_make_capsmap(&[("demo::rvs_seeded_BIS", "S")]);

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let expected: BTreeMap<_, _> = diffs
            .iter()
            .map(|diff| (diff.def_path.rvs_as_str(), diff.expected_name.rvs_as_str()))
            .collect();
        let output = format!(
            "caller={}\nseeded={}\n",
            expected
                .get("demo::rvs_call_PS")
                .copied()
                .unwrap_or("missing"),
            expected
                .get("demo::rvs_seeded_BIS")
                .copied()
                .unwrap_or("missing"),
        );
        rvs_snapshot_BIS(
            "test_20260715_incomplete_inference_stops_at_authoritative_boundaries",
            &output,
        );

        assert_eq!(expected.get("demo::rvs_call_PS"), Some(&"rvs_call_P"));
        assert_eq!(expected.get("demo::rvs_seeded_BIS"), Some(&"rvs_seeded_S"));
    }

    #[test]
    fn test_20260715_incomplete_trait_dispatch_taints_declaration() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch_BIS"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let mut implementation = rvs_make_behavior();
        implementation.is_trait_impl = true;
        implementation.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryFetcher::rvs_fetch_BIS@demo::Fetcher"),
            implementation,
        );

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let trait_path = DefPath::from("demo::Fetcher::rvs_fetch_BIS");
        let implementation_path = DefPath::from("demo::MemoryFetcher::rvs_fetch_BIS@demo::Fetcher");
        let diff = analysis
            .diffs
            .iter()
            .find(|diff| diff.def_path == trait_path)
            .expect("never: local trait declaration produces a contract diff");
        let output = format!(
            "trait_incomplete={}\nimplementation_incomplete={}\nexpected={}\nname_mismatch={}\n",
            analysis.rvs_incomplete_paths().contains(&trait_path),
            analysis
                .rvs_incomplete_paths()
                .contains(&implementation_path),
            diff.expected_name,
            diff.rvs_has_name_mismatch(),
        );
        rvs_snapshot_BIS(
            "test_20260715_incomplete_trait_dispatch_taints_declaration",
            &output,
        );

        assert!(analysis.rvs_incomplete_paths().contains(&trait_path));
        assert!(!diff.rvs_has_name_mismatch());
    }

    #[test]
    fn test_20260715_bodyless_unannotated_callee_remains_unknown() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("dep::DavFileSystem::open"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::DavFileSystem::open"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_open"), caller);
        let inference = PreparedInference::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let seed = capsmap::CapsMap::rvs_new();
        let resolver = inference.rvs_resolver(&graph, &seed);
        let callee = DefPath::from("dep::DavFileSystem::open");
        let propagation = resolver.rvs_for_propagation_target(&callee);
        let contract = resolver.rvs_for_contract_check(&callee);
        let explanation = resolver.rvs_for_explanation_view(&callee);
        let output = format!(
            "propagation={propagation:?}\ncontract={contract:?}\nexplanation={explanation:?}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_bodyless_unannotated_callee_remains_unknown",
            &output,
        );

        assert!(propagation.is_none());
        assert!(contract.is_none());
        assert!(explanation.is_none());
    }

    #[test]
    fn test_20260718_direct_external_dep_emits_incomplete_wrapper_caps() {
        let mut graph = FnGraph::rvs_new();
        let wrapper_path = DefPath::from("dep::log");
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: wrapper_path.clone(),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut wrapper = rvs_make_behavior();
        wrapper.facts.has_static_ref = true;
        wrapper.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::Log::log"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(wrapper_path.clone(), wrapper);
        graph.rvs_insert_M(
            DefPath::from("dep::Log::log"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let rendered = rvs_format_def_path_capability_info(&known);
        let output = format!(
            "wrapper_incomplete={}\nknown={}\nunknown={}\nrendered={rendered}",
            inference.rvs_incomplete_paths().contains(&wrapper_path),
            known.contains_key(&wrapper_path),
            unknown.contains_key(&wrapper_path),
        );
        rvs_snapshot_BIS(
            "test_20260718_direct_external_dep_emits_incomplete_wrapper_caps",
            &output,
        );

        let info = known
            .get(&wrapper_path)
            .expect("never: resolvable wrapper emits its known lower bound");
        assert!(!unknown.contains_key(&wrapper_path));
        assert_eq!(info.rvs_caps().rvs_letters(), "S");
        assert_eq!(info.rvs_basis(), &CapabilityBasis::Inferred);
        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Incomplete);
    }

    #[test]
    fn test_20260729_direct_external_deps_use_primary_target_calls() {
        let caller_path = DefPath::from("demo::rvs_shared_helper");
        let local_callee = DefPath::from("demo::rvs_local_only");
        let dependency_callee = DefPath::from("dependency::effect");
        let mut caller = rvs_make_behavior();
        caller.calls = BTreeMap::from([
            (
                FunctionIdentity {
                    crate_id: 2,
                    def_path: local_callee.clone(),
                },
                CallEdgeType::Strong,
            ),
            (
                FunctionIdentity {
                    crate_id: 2,
                    def_path: dependency_callee.clone(),
                },
                CallEdgeType::Strong,
            ),
        ]);
        let local_target = caller.rvs_test_target_M(10);
        local_target.crate_provenance = crate::artifacts::CrateProvenance::PrimaryPackage;
        local_target.calls.insert(
            crate::artifacts::FunctionIdentity {
                crate_id: 10,
                def_path: local_callee,
            },
            CallEdgeType::Strong,
        );
        let dependency_target = caller.rvs_test_target_M(20);
        dependency_target.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        dependency_target.calls.insert(
            crate::artifacts::FunctionIdentity {
                crate_id: 30,
                def_path: dependency_callee.clone(),
            },
            CallEdgeType::Strong,
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(caller_path, caller);
        let mut dependency = rvs_make_behavior();
        dependency.facts.has_static_ref = true;
        dependency.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        graph.rvs_insert_M(dependency_callee.clone(), dependency);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let output = format!(
            "dependency_known={}\ndependency_unknown={}\n",
            known.contains_key(&dependency_callee),
            unknown.contains_key(&dependency_callee),
        );
        rvs_snapshot_BIS(
            "test_20260729_direct_external_deps_use_primary_target_calls",
            &output,
        );

        assert!(!known.contains_key(&dependency_callee));
        assert!(!unknown.contains_key(&dependency_callee));
    }

    #[test]
    fn test_20260718_direct_external_trait_dispatch_emits_incomplete_vote() {
        let mut graph = FnGraph::rvs_new();
        let dispatch_path = DefPath::from("dep::Fetcher::rvs_fetch_S");
        let implementation_path = DefPath::from("dep::MemoryFetcher::rvs_fetch_S@dep::Fetcher");

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: dispatch_path.clone(),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_S"), caller);
        graph.rvs_insert_M(
            dispatch_path.clone(),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let mut implementation = rvs_make_behavior();
        implementation.is_trait_impl = true;
        implementation.facts.has_static_ref = true;
        implementation.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(implementation_path.clone(), implementation);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let rendered = rvs_format_def_path_capability_info(&known);
        let output = format!(
            "dispatch_incomplete={}\nimplementation_incomplete={}\nknown={}\nunknown={}\nrendered={rendered}",
            inference.rvs_incomplete_paths().contains(&dispatch_path),
            inference
                .rvs_incomplete_paths()
                .contains(&implementation_path),
            known.contains_key(&dispatch_path),
            unknown.contains_key(&dispatch_path),
        );
        rvs_snapshot_BIS(
            "test_20260718_direct_external_trait_dispatch_emits_incomplete_vote",
            &output,
        );

        let info = known
            .get(&dispatch_path)
            .expect("never: resolvable trait vote emits its known lower bound");
        assert!(!unknown.contains_key(&dispatch_path));
        assert_eq!(info.rvs_caps().rvs_letters(), "S");
        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Incomplete);
        let CapabilityBasis::TraitVote {
            implementations,
            threshold,
            votes,
        } = info.rvs_basis()
        else {
            panic!("expected trait vote basis, got {:?}", info.rvs_basis());
        };
        assert_eq!((*implementations, *threshold), (1, 1));
        assert_eq!(votes, &BTreeMap::from([(Capability::S, 1)]));
    }

    #[test]
    fn test_20260728_provided_trait_body_taints_complete_override_vote() {
        let mut graph = FnGraph::rvs_new();
        let dispatch_path = DefPath::from("dep::Fetcher::rvs_fetch_S");
        let implementation_path = DefPath::from("dep::MemoryFetcher::rvs_fetch_S@dep::Fetcher");

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: dispatch_path.clone(),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_S"), caller);

        let mut provided_method = rvs_make_behavior();
        provided_method.facts.has_static_ref = true;
        provided_method.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::unknown"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(dispatch_path.clone(), provided_method);

        let mut implementation = rvs_make_behavior();
        implementation.is_trait_impl = true;
        graph.rvs_insert_M(implementation_path.clone(), implementation);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let aliases = rvs_generate_trait_alias_infos(
            inference.rvs_inferred(),
            inference.rvs_impl_index(),
            &graph,
            inference.rvs_incomplete_paths(),
        );
        let direct_rendered = rvs_format_def_path_capability_info(&known);
        let alias_rendered = rvs_format_def_path_capability_info(&aliases);
        let output = format!(
            "dispatch_incomplete={}\nimplementation_incomplete={}\nknown={}\nunknown={}\ndirect={direct_rendered}alias={alias_rendered}",
            inference.rvs_incomplete_paths().contains(&dispatch_path),
            inference
                .rvs_incomplete_paths()
                .contains(&implementation_path),
            known.contains_key(&dispatch_path),
            unknown.contains_key(&dispatch_path),
        );
        rvs_snapshot_BIS(
            "test_20260728_provided_trait_body_taints_complete_override_vote",
            &output,
        );

        let info = known
            .get(&dispatch_path)
            .expect("never: resolvable provided trait vote emits its known lower bound");
        assert!(!unknown.contains_key(&dispatch_path));
        assert_eq!(info.rvs_caps().rvs_letters(), "S");
        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Incomplete);
        assert_eq!(info.rvs_basis(), &CapabilityBasis::Inferred);
        assert_eq!(aliases.get(&dispatch_path), Some(info));
        let vote = inference
            .rvs_trait_votes()
            .get(&dispatch_path)
            .expect("never: provided trait method has an override vote");
        assert!(vote.selected_caps.rvs_is_empty());
        assert!(vote.rvs_is_complete());
    }

    #[test]
    fn test_20260728_bodyless_trait_alias_preserves_signature_lower_bound() {
        let mut graph = FnGraph::rvs_new();
        let dispatch_path = DefPath::from("dep::Transformer::rvs_transform_AMU");
        let implementation_path =
            DefPath::from("dep::MemoryTransformer::rvs_transform_AMU@dep::Transformer");

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: dispatch_path.clone(),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_S"), caller);

        let mut required_method = rvs_make_behavior();
        required_method.has_body = false;
        required_method.facts.has_async = true;
        required_method.facts.has_mut_param = true;
        required_method.facts.is_unsafe_fn = true;
        graph.rvs_insert_M(dispatch_path.clone(), required_method);

        let mut implementation = rvs_make_behavior();
        implementation.is_trait_impl = true;
        implementation.facts.has_static_ref = true;
        graph.rvs_insert_M(implementation_path, implementation);

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let aliases = rvs_generate_trait_alias_infos(
            inference.rvs_inferred(),
            inference.rvs_impl_index(),
            &graph,
            inference.rvs_incomplete_paths(),
        );
        let direct_rendered = rvs_format_def_path_capability_info(&known);
        let alias_rendered = rvs_format_def_path_capability_info(&aliases);
        let output = format!(
            "dispatch_incomplete={}\nknown={}\nunknown={}\ndirect={direct_rendered}alias={alias_rendered}",
            inference.rvs_incomplete_paths().contains(&dispatch_path),
            known.contains_key(&dispatch_path),
            unknown.contains_key(&dispatch_path),
        );
        rvs_snapshot_BIS(
            "test_20260728_bodyless_trait_alias_preserves_signature_lower_bound",
            &output,
        );

        let info = known
            .get(&dispatch_path)
            .expect("never: bodyless trait method emits signature and vote lower bounds");
        assert!(!unknown.contains_key(&dispatch_path));
        assert_eq!(info.rvs_caps().rvs_letters(), "AMSU");
        assert_eq!(info.rvs_basis(), &CapabilityBasis::Inferred);
        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Complete);
        assert_eq!(aliases.get(&dispatch_path), Some(info));
    }

    #[test]
    fn test_20260712_prepare_inference_builds_shared_derivatives_once() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let mut port_method = rvs_make_behavior();
        port_method.facts.is_port_method = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Service::rvs_load@demo::Repository"),
            port_method,
        );
        let mut seed = capsmap::CapsMap::rvs_new();
        seed.rvs_insert_M(
            CapsMapKey::from("dep::read"),
            CapabilitySet::rvs_from_validated("BI"),
        );

        let inference = PreparedInference::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let run_caps = inference
            .rvs_inferred()
            .get(&DefPath::from("demo::rvs_run"))
            .map(rvs_caps_to_string)
            .unwrap_or_default();
        let port_caps = inference
            .rvs_inferred()
            .get(&DefPath::from("demo::Service::rvs_load@demo::Repository"))
            .map(rvs_caps_to_string)
            .unwrap_or_default();
        let output = format!(
            "inferred={}\nimpl_keys={}\nsynthetic={}\nrun_caps={run_caps}\nport_caps={port_caps}\n",
            inference.rvs_inferred().len(),
            inference.rvs_impl_index().len(),
            inference.rvs_synthetic_paths().len(),
        );
        rvs_snapshot_BIS(
            "test_20260712_prepare_inference_builds_shared_derivatives_once",
            &output,
        );

        assert_eq!(run_caps, "BI");
        assert_eq!(port_caps, "P");
        assert_eq!(inference.rvs_impl_index().len(), 1);
        assert_eq!(
            inference.rvs_synthetic_paths(),
            &BTreeSet::from([
                DefPath::from("dep::read"),
                DefPath::from("demo::Repository::rvs_load"),
            ])
        );
    }

    fn rvs_infer_caps_case_M(
        entries: &[(&str, FnNode)],
        seed_entries: &[(&str, &str)],
    ) -> BTreeMap<DefPath, CapabilitySet> {
        let mut graph = FnGraph::rvs_new();
        for (path, behavior) in entries {
            graph.rvs_insert_M(DefPath::from(*path), behavior.clone());
        }
        let seed = rvs_make_capsmap(seed_entries);
        rvs_infer_caps(&graph, &seed)
    }

    // ─── rvs_infer_caps ──────────────────────────────────────────────────

    #[test]
    fn test_20260609_infer_caps_empty_callgraph() {
        let graph = FnGraph::rvs_new();
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps(&graph, &seed);
        rvs_snapshot_BIS(
            "test_20260609_infer_caps_empty_callgraph",
            &format!("{result:?}"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_20260704_infer_caps_propagates_deep_chain_to_fixed_point() {
        let mut graph = FnGraph::rvs_new();
        for i in 0..=20 {
            let mut node = rvs_make_behavior();
            let callee = if i == 20 {
                DefPath::from("std::fs::read_to_string")
            } else {
                DefPath::from(format!("demo::rvs_f{:02}", i + 1))
            };
            node.calls.insert(
                FunctionIdentity {
                    crate_id: 2,
                    def_path: callee,
                },
                CallEdgeType::Strong,
            );
            graph.rvs_insert_M(DefPath::from(format!("demo::rvs_f{i:02}")), node);
        }
        let seed = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);

        let result = rvs_infer_caps(&graph, &seed);
        let top_caps = result
            .get("demo::rvs_f00")
            .expect("top function should be inferred");
        let output = format!("top_caps={}\n", rvs_caps_to_string(top_caps));
        rvs_snapshot_BIS(
            "test_20260704_infer_caps_propagates_deep_chain_to_fixed_point",
            &output,
        );

        assert!(top_caps.rvs_contains(Capability::B));
        assert!(top_caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260704_infer_caps_uses_absent_rvs_callee_suffix() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_make_behavior();
        node.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::rvs_write_BI"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let seed = capsmap::CapsMap::rvs_new();

        let inferred = rvs_infer_caps(&graph, &seed);
        let impl_index = rvs_build_impl_index(&graph);
        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &BTreeSet::from([CrateName::from("demo")]),
            &seed,
            &inferred,
            &impl_index,
        );
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let output = format!(
            "run_caps={}\nknown={known:?}\nunknown={unknown:?}\n",
            rvs_caps_to_string(run_caps),
        );
        rvs_snapshot_BIS(
            "test_20260704_infer_caps_uses_absent_rvs_callee_suffix",
            &output,
        );

        assert!(run_caps.rvs_contains(Capability::B));
        assert!(run_caps.rvs_contains(Capability::I));
        assert!(known.contains_key("dep::rvs_write_BI"));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260705_infer_caps_uses_known_caps_from_mixed_unknown_suffix() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::rvs_send_AEIS"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let inferred = rvs_infer_caps(&graph, &capsmap::CapsMap::rvs_new());
        let caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        rvs_snapshot_BIS(
            "test_20260705_infer_caps_uses_known_caps_from_mixed_unknown_suffix",
            &format!("caps={}\n", rvs_caps_to_string(caps)),
        );

        assert!(caps.rvs_contains(Capability::I));
        assert!(caps.rvs_contains(Capability::S));
    }

    #[test]
    fn test_20260704_declared_caps_from_def_path_handles_trait_impl_suffix() {
        let caps = rvs_declared_caps_from_def_path(&DefPath::from(
            "demo::Adapter::rvs_fetch_BI@demo::ApiClient",
        ))
        .expect("rvs trait impl method should declare caps");
        let none = rvs_declared_caps_from_def_path(&DefPath::from("demo::fetch"));
        let unknown = rvs_declared_caps_from_def_path(&DefPath::from("demo::rvs_fetch_E"));
        let output = format!("caps={}\nnone={none:?}\n", rvs_caps_to_string(&caps));
        rvs_snapshot_BIS(
            "test_20260704_declared_caps_from_def_path_handles_trait_impl_suffix",
            &output,
        );

        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(none.is_none());
        assert!(unknown.is_none());
    }

    #[test]
    fn test_20260704_infer_caps_uses_impl_caps_for_bodyless_trait_decl() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::Fetcher::rvs_fetch"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let mut impl_method = rvs_make_behavior();
        impl_method.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::DiskFetcher::rvs_fetch@demo::Fetcher"),
            impl_method,
        );

        let seed = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);
        let inferred = rvs_infer_caps(&graph, &seed);
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let decl_caps = inferred
            .get("demo::Fetcher::rvs_fetch")
            .expect("bodyless declaration should be inferred");
        let output = format!("run_caps={}\n", rvs_caps_to_string(run_caps));
        rvs_snapshot_BIS(
            "test_20260704_infer_caps_uses_impl_caps_for_bodyless_trait_decl",
            &output,
        );

        assert!(run_caps.rvs_contains(Capability::B));
        assert!(run_caps.rvs_contains(Capability::I));
        assert!(decl_caps.rvs_contains(Capability::B));
        assert!(decl_caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260706_local_trait_decl_suffix_loses_to_impl_vote() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryFetcher::rvs_fetch_BI@demo::Fetcher"),
            rvs_make_behavior(),
        );

        let inferred = rvs_infer_caps(&graph, &capsmap::CapsMap::rvs_new());
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let decl_caps = inferred
            .get("demo::Fetcher::rvs_fetch_BI")
            .expect("bodyless declaration should be inferred");
        rvs_snapshot_BIS(
            "test_20260706_local_trait_decl_suffix_loses_to_impl_vote",
            &format!(
                "run={}\ndecl={}\n",
                rvs_caps_to_string(run_caps),
                rvs_caps_to_string(decl_caps)
            ),
        );

        assert!(run_caps.rvs_is_empty());
        assert!(decl_caps.rvs_is_empty());
    }

    #[test]
    fn test_20260706_bodyless_trait_decl_keeps_signature_caps_with_impl_vote() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::Fetcher::rvs_fetch_A"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.has_async = true;
        graph.rvs_insert_M(DefPath::from("demo::Fetcher::rvs_fetch_A"), trait_decl);
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryFetcher::rvs_fetch_A@demo::Fetcher"),
            rvs_make_behavior(),
        );

        let inferred = rvs_infer_caps(&graph, &capsmap::CapsMap::rvs_new());
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let decl_caps = inferred
            .get("demo::Fetcher::rvs_fetch_A")
            .expect("bodyless declaration should be inferred");
        rvs_snapshot_BIS(
            "test_20260706_bodyless_trait_decl_keeps_signature_caps_with_impl_vote",
            &format!(
                "run={}\ndecl={}\n",
                rvs_caps_to_string(run_caps),
                rvs_caps_to_string(decl_caps)
            ),
        );

        assert!(run_caps.rvs_is_empty());
        assert!(decl_caps.rvs_contains(Capability::A));
    }

    #[test]
    fn test_20260705_bodyless_port_decl_stays_port_caps() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::ApiClient::rvs_fetch_P"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let mut impl_method = rvs_make_behavior();
        impl_method.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::DiskClient::rvs_fetch_P@demo::ApiClient"),
            impl_method,
        );

        let seed = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);
        let inferred = rvs_infer_caps(&graph, &seed);
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let decl_caps = inferred
            .get("demo::ApiClient::rvs_fetch_P")
            .expect("trait declaration should be inferred");
        rvs_snapshot_BIS(
            "test_20260705_bodyless_port_decl_stays_port_caps",
            &format!(
                "run={}\ndecl={}\n",
                rvs_caps_to_string(run_caps),
                rvs_caps_to_string(decl_caps)
            ),
        );

        assert!(run_caps.rvs_contains(Capability::P));
        assert!(!run_caps.rvs_contains(Capability::B));
        assert!(!run_caps.rvs_contains(Capability::I));
        assert!(decl_caps.rvs_contains(Capability::P));
    }

    #[test]
    fn test_20260705_port_method_caps_ignore_stale_capsmap() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::ApiClient::rvs_fetch_P"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let seed = rvs_make_capsmap(&[("demo::ApiClient::rvs_fetch_P", "BI")]);
        let inferred = rvs_infer_caps(&graph, &seed);
        let run_caps = inferred
            .get("demo::rvs_run")
            .expect("caller should be inferred");
        let decl_caps = inferred
            .get("demo::ApiClient::rvs_fetch_P")
            .expect("trait declaration should be inferred");
        rvs_snapshot_BIS(
            "test_20260705_port_method_caps_ignore_stale_capsmap",
            &format!(
                "run={}\ndecl={}\n",
                rvs_caps_to_string(run_caps),
                rvs_caps_to_string(decl_caps)
            ),
        );

        assert!(run_caps.rvs_contains(Capability::P));
        assert!(!run_caps.rvs_contains(Capability::B));
        assert!(!run_caps.rvs_contains(Capability::I));
        assert!(decl_caps.rvs_contains(Capability::P));
        assert!(!decl_caps.rvs_contains(Capability::B));
        assert!(!decl_caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260704_resolve_callee_caps_prefers_impl_for_bodyless_decl() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::DiskFetcher::rvs_fetch@demo::Fetcher"),
            rvs_make_behavior(),
        );
        let impl_index = rvs_build_impl_index(&graph);
        let inferred = BTreeMap::from([
            (
                DefPath::from("demo::Fetcher::rvs_fetch"),
                CapabilitySet::rvs_new(),
            ),
            (
                DefPath::from("demo::DiskFetcher::rvs_fetch@demo::Fetcher"),
                CapabilitySet::rvs_from_validated("BI"),
            ),
        ]);

        let seed = capsmap::CapsMap::rvs_new();
        let caps = CalleeCapsResolver::rvs_new(&graph, &seed, &inferred, &impl_index)
            .rvs_for_propagation_target(&DefPath::from("demo::Fetcher::rvs_fetch"))
            .expect("bodyless declaration should resolve through impl aggregation");
        let output = format!("caps={}\n", rvs_caps_to_string(&caps));
        rvs_snapshot_BIS(
            "test_20260704_resolve_callee_caps_prefers_impl_for_bodyless_decl",
            &output,
        );

        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260704_resolve_callee_caps_uses_declared_suffix_for_bodyless_decl_without_impl() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        let impl_index = rvs_build_impl_index(&graph);
        let inferred = BTreeMap::from([(
            DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            CapabilitySet::rvs_new(),
        )]);

        let seed = capsmap::CapsMap::rvs_new();
        let caps = CalleeCapsResolver::rvs_new(&graph, &seed, &inferred, &impl_index)
            .rvs_for_propagation_target(&DefPath::from("demo::Fetcher::rvs_fetch_BI"))
            .expect("bodyless declaration should use its declared suffix when no impls exist");
        let output = format!("caps={}\n", rvs_caps_to_string(&caps));
        rvs_snapshot_BIS(
            "test_20260704_resolve_callee_caps_uses_declared_suffix_for_bodyless_decl_without_impl",
            &output,
        );

        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260710_callee_caps_resolver_policy_table() {
        #[derive(Debug)]
        struct Case<'a> {
            name: &'a str,
            callee: &'a str,
            propagation: Option<&'a str>,
            contract: Option<&'a str>,
            explanation: Option<&'a str>,
        }

        let mut graph = FnGraph::rvs_new();
        let mut port = rvs_make_behavior();
        port.has_body = false;
        port.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), port);

        graph.rvs_insert_M(DefPath::from("demo::rvs_exact_S"), rvs_make_behavior());
        graph.rvs_insert_M(DefPath::from("demo::rvs_mixed_AEIS"), rvs_make_behavior());

        let mut bodyless = rvs_make_behavior();
        bodyless.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::Fetcher::rvs_fetch_A"), bodyless);
        graph.rvs_insert_M(
            DefPath::from("demo::DiskFetcher::rvs_fetch_A@demo::Fetcher"),
            rvs_make_behavior(),
        );

        graph.rvs_insert_M(DefPath::from("demo::rvs_synthetic"), rvs_make_behavior());
        graph.rvs_insert_M(
            DefPath::from("demo::Disk::rvs_read_BI@demo::Reader"),
            rvs_make_behavior(),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Disk::read@demo::Reader"),
            rvs_make_behavior(),
        );

        let caps = rvs_make_capsmap(&[
            ("demo::ApiClient::rvs_fetch_P", "BI"),
            ("demo::rvs_exact_S", "BI"),
        ]);
        let inferred = BTreeMap::from([
            (
                DefPath::from("demo::ApiClient::rvs_fetch_P"),
                CapabilitySet::rvs_from_validated("S"),
            ),
            (
                DefPath::from("demo::rvs_exact_S"),
                CapabilitySet::rvs_from_validated("T"),
            ),
            (
                DefPath::from("demo::rvs_mixed_AEIS"),
                CapabilitySet::rvs_from_validated("T"),
            ),
            (
                DefPath::from("demo::Fetcher::rvs_fetch_A"),
                CapabilitySet::rvs_from_validated("AMU"),
            ),
            (
                DefPath::from("demo::DiskFetcher::rvs_fetch_A@demo::Fetcher"),
                CapabilitySet::rvs_from_validated("BI"),
            ),
            (
                DefPath::from("demo::rvs_synthetic"),
                CapabilitySet::rvs_from_validated("S"),
            ),
            (
                DefPath::from("demo::Disk::rvs_read_BI@demo::Reader"),
                CapabilitySet::rvs_from_validated("S"),
            ),
            (
                DefPath::from("demo::Disk::read@demo::Reader"),
                CapabilitySet::rvs_from_validated("BI"),
            ),
        ]);
        let impl_index = rvs_build_impl_index(&graph);
        let resolver = CalleeCapsResolver::rvs_new(&graph, &caps, &inferred, &impl_index);
        let cases = [
            Case {
                name: "port_method",
                callee: "demo::ApiClient::rvs_fetch_P",
                propagation: Some("PS"),
                contract: Some("PS"),
                explanation: Some("S"),
            },
            Case {
                name: "exact_capsmap_override",
                callee: "demo::rvs_exact_S",
                propagation: Some("BI"),
                contract: Some("BI"),
                explanation: Some("T"),
            },
            Case {
                name: "mixed_unknown_declared_suffix",
                callee: "demo::rvs_mixed_AEIS",
                propagation: Some("T"),
                contract: Some("AIS"),
                explanation: Some("T"),
            },
            Case {
                name: "unknown_only_suffix",
                callee: "dep::rvs_external_E",
                propagation: None,
                contract: None,
                explanation: None,
            },
            Case {
                name: "bodyless_impl_merges_signature",
                callee: "demo::Fetcher::rvs_fetch_A",
                propagation: Some("ABIMU"),
                contract: Some("A"),
                explanation: Some("AMU"),
            },
            Case {
                name: "synthetic_node",
                callee: "demo::rvs_synthetic",
                propagation: Some("S"),
                contract: Some(""),
                explanation: Some("S"),
            },
            Case {
                name: "external_mixed_suffix",
                callee: "dep::rvs_external_BX",
                propagation: Some("B"),
                contract: Some("B"),
                explanation: None,
            },
            Case {
                name: "impl_trait_path",
                callee: "demo::Disk::rvs_read_BI@demo::Reader",
                propagation: Some("S"),
                contract: Some("BI"),
                explanation: Some("S"),
            },
            Case {
                name: "absent_impl_trait_path",
                callee: "dep::Disk::rvs_read_BI@dep::Reader",
                propagation: Some("BI"),
                contract: Some("BI"),
                explanation: None,
            },
            Case {
                name: "trait_impl_majority_fallback",
                callee: "demo::Reader::read",
                propagation: Some("BI"),
                contract: Some("BI"),
                explanation: Some("BI"),
            },
        ];

        let mut output = String::new();
        for case in cases {
            let callee = DefPath::from(case.callee);
            let propagation = resolver.rvs_for_propagation_target(&callee);
            let contract = resolver.rvs_for_contract_check(&callee);
            let explanation = resolver.rvs_for_explanation_view(&callee);
            let rendered = |value: &Option<CapabilitySet>| {
                value
                    .as_ref()
                    .map(rvs_caps_to_string)
                    .unwrap_or_else(|| "unknown".to_string())
            };
            output.push_str(&format!(
                "{}: propagation={} contract={} explanation={}\n",
                case.name,
                rendered(&propagation),
                rendered(&contract),
                rendered(&explanation),
            ));
            assert_eq!(
                propagation.as_ref().map(rvs_caps_to_string).as_deref(),
                case.propagation,
                "{} propagation policy",
                case.name
            );
            assert_eq!(
                contract.as_ref().map(rvs_caps_to_string).as_deref(),
                case.contract,
                "{} contract policy",
                case.name
            );
            assert_eq!(
                explanation.as_ref().map(rvs_caps_to_string).as_deref(),
                case.explanation,
                "{} explanation policy",
                case.name
            );
        }
        rvs_snapshot_BIS("test_20260710_callee_caps_resolver_policy_table", &output);
    }

    #[test]
    fn test_20260703_infer_graph_sets_node_caps() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let caps = analysis.rvs_inferred().get(&DefPath::from("demo::rvs_run"));
        rvs_snapshot_BIS(
            "test_20260703_infer_graph_sets_node_caps",
            &format!("caps={caps:?}\n"),
        );

        assert!(caps.is_some());
        assert!(analysis.rvs_synthetic_paths().is_empty());
    }

    #[test]
    fn test_20260710_infer_graph_returns_installed_caps_map() {
        let mut run = rvs_make_behavior();
        run.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), run);
        let seed = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let mut output = String::new();
        for (path, caps) in analysis.rvs_inferred() {
            output.push_str(&format!(
                "{path}: inferred={} synthetic={} graph_contains={}\n",
                rvs_caps_to_string(caps),
                analysis.rvs_synthetic_paths().contains(path),
                graph.rvs_get(path.rvs_as_str()).is_some(),
            ));
        }
        rvs_snapshot_BIS(
            "test_20260710_infer_graph_returns_installed_caps_map",
            &output,
        );

        assert_eq!(analysis.rvs_inferred().len(), 2);
        assert!(
            analysis
                .rvs_synthetic_paths()
                .contains(&DefPath::from("std::fs::read_to_string"))
        );
        assert!(graph.rvs_get("std::fs::read_to_string").is_none());
    }

    #[test]
    fn test_20260704_infer_graph_prunes_stale_synthetic_nodes() {
        let mut graph = FnGraph::rvs_new();
        let mut run = rvs_make_behavior();
        run.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), run);
        let seed = rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]);
        let first = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        assert!(
            first
                .rvs_synthetic_paths()
                .contains(&DefPath::from("std::fs::read_to_string"))
        );

        graph
            .rvs_get_mut_M(&DefPath::from("demo::rvs_run"))
            .expect("demo node should exist")
            .calls
            .clear();
        let second = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );

        let has_synthetic = second
            .rvs_synthetic_paths()
            .contains(&DefPath::from("std::fs::read_to_string"));
        let run_caps = second
            .rvs_inferred()
            .get(&DefPath::from("demo::rvs_run"))
            .expect("demo node should keep inferred caps");
        let output = format!(
            "has_synthetic={has_synthetic}\nrun_caps={}\n",
            rvs_caps_to_string(&run_caps),
        );
        rvs_snapshot_BIS(
            "test_20260704_infer_graph_prunes_stale_synthetic_nodes",
            &output,
        );

        assert!(!has_synthetic);
        assert!(run_caps.rvs_is_empty());
    }

    #[test]
    fn test_20260703_project_expected_local_names_sets_expected_name() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::parse"), rvs_make_behavior());
        let inferred = BTreeMap::from([(
            DefPath::from("demo::parse"),
            CapabilitySet::rvs_from_validated("BI"),
        )]);

        let diffs = rvs_collect_contract_diffs(
            &graph,
            &inferred,
            &BTreeSet::from([CrateName::from("demo")]),
        );

        let expected_name = diffs
            .first()
            .map(|diff| diff.expected_name.rvs_as_str())
            .unwrap_or("");
        rvs_snapshot_BIS(
            "test_20260703_project_expected_local_names_sets_expected_name",
            &format!("expected_name={expected_name}\n"),
        );

        assert_eq!(expected_name, "rvs_parse_BI");
    }

    #[test]
    fn test_20260704_project_expected_local_names_preserves_pure_suffix_like_name() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::parse_BI"), rvs_make_behavior());
        let inferred =
            BTreeMap::from([(DefPath::from("demo::parse_BI"), CapabilitySet::rvs_new())]);

        let diffs = rvs_collect_contract_diffs(
            &graph,
            &inferred,
            &BTreeSet::from([CrateName::from("demo")]),
        );

        let expected_name = diffs
            .first()
            .map(|diff| diff.expected_name.rvs_as_str())
            .unwrap_or("");
        rvs_snapshot_BIS(
            "test_20260704_project_expected_local_names_preserves_pure_suffix_like_name",
            &format!("expected_name={expected_name}\n"),
        );

        assert_eq!(expected_name, "rvs_parse_BI");
    }

    #[test]
    fn test_20260704_contract_base_name_strips_rvs_and_suffix_like_names() {
        let cases = [
            ("rvs_parse_BI", "BI", "parse"),
            ("parse_BI", "BI", "parse"),
            ("parse_BI", "", "parse_BI"),
            ("fetch_BI", "P", "fetch"),
            ("parse_JSON", "", "parse_JSON"),
            ("parse", "", "parse"),
            ("parse_json", "", "parse_json"),
        ];
        let output = cases
            .iter()
            .map(|(input, caps, _)| {
                format!("{input}/{caps} -> {}", rvs_contract_base_name(input, caps))
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260704_contract_base_name_strips_rvs_and_suffix_like_names",
            &output,
        );

        for (input, caps, expected) in cases {
            assert_eq!(rvs_contract_base_name(input, caps), expected);
        }
    }

    #[test]
    fn test_20260704_project_expected_local_names_flags_non_lowercase_names() {
        let mut graph = FnGraph::rvs_new();
        for name in ["demo::Foo", "demo::_helper"] {
            graph.rvs_insert_M(DefPath::from(name), rvs_make_behavior());
        }

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = diffs
            .iter()
            .map(|diff| {
                format!(
                    "{} -> {:?} {:?}",
                    diff.actual_name,
                    diff.expected_name,
                    diff.rvs_mismatch_kinds(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260704_project_expected_local_names_flags_non_lowercase_names",
            &output,
        );

        assert!(diffs.iter().any(|diff| {
            diff.actual_name.rvs_as_str() == "Foo"
                && diff.expected_name.rvs_as_str() == "rvs_Foo"
                && diff.rvs_missing_rvs_prefix()
        }));
        assert!(diffs.iter().any(|diff| {
            diff.actual_name.rvs_as_str() == "_helper"
                && diff.expected_name.rvs_as_str() == "rvs__helper"
                && diff.rvs_missing_rvs_prefix()
        }));
    }

    #[test]
    fn test_20260703_collect_local_contract_diffs_updates_existing_rvs_suffix() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_fetch_ABI"),
            FnNode {
                facts: CapabilityFacts {
                    is_port_method: true,
                    ..CapabilityFacts::default()
                },
                sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                    "src/lib.rs".into(),
                    1,
                    2,
                )]),
                ..FnNode::default()
            },
        );

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = diffs.first().expect("expected one local contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_local_contract_diffs_updates_existing_rvs_suffix",
            &format!(
                "diff={diff:?}\nnode={:?}\n",
                graph.rvs_get("demo::rvs_fetch_ABI")
            ),
        );

        assert_eq!(diff.expected_name.rvs_as_str(), "rvs_fetch_P");
        assert!(diff.rvs_has_name_mismatch());
    }

    #[test]
    fn test_20260706_local_trait_decl_expected_name_uses_impl_vote() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            FnNode {
                has_body: false,
                sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                    "src/lib.rs".into(),
                    1,
                    2,
                )]),
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryFetcher::rvs_fetch_BI@demo::Fetcher"),
            FnNode {
                is_trait_impl: true,
                ..rvs_make_behavior()
            },
        );

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = diffs.first().expect("expected trait declaration diff");
        let diff_paths: Vec<_> = diffs
            .iter()
            .map(|diff| diff.def_path.rvs_as_str())
            .collect();
        rvs_snapshot_BIS(
            "test_20260706_local_trait_decl_expected_name_uses_impl_vote",
            &format!(
                "actual={}\nexpected={:?}\ncaps={:?}\ndiff_paths={diff_paths:?}\n",
                diff.actual_name, diff.expected_name, diff.expected_public_caps,
            ),
        );

        assert_eq!(diffs.len(), 1);
        assert_eq!(diff.expected_name.rvs_as_str(), "rvs_fetch");
        assert!(diff.expected_public_caps.rvs_is_empty());
    }

    #[test]
    fn test_20260703_collect_contract_diffs_reports_name_and_caps_mismatch() {
        let path = DefPath::from("demo::rvs_fetch_ABI");
        let inferred = BTreeMap::from([(path.clone(), CapabilitySet::rvs_from_validated("P"))]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(path, rvs_make_behavior());
        let diffs = rvs_collect_contract_diffs(
            &graph,
            &inferred,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = diffs.first().expect("expected one contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_contract_diffs_reports_name_and_caps_mismatch",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(diff.actual_name.rvs_as_str(), "rvs_fetch_ABI");
        assert!(diff.rvs_has_name_mismatch());
        assert_eq!(
            diff.expected_public_caps,
            CapabilitySet::rvs_from_validated("P")
        );
        assert_eq!(
            diff.declared_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("ABI"))
        );
    }

    #[test]
    fn test_20260703_collect_contract_diffs_reads_trait_impl_method_name() {
        let path = DefPath::from("demo::Adapter::rvs_fetch_BI@demo::Client");
        let inferred = BTreeMap::from([(path.clone(), CapabilitySet::rvs_from_validated("P"))]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            path,
            FnNode {
                is_trait_impl: true,
                ..FnNode::default()
            },
        );
        let diffs = rvs_collect_contract_diffs(
            &graph,
            &inferred,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        rvs_snapshot_BIS(
            "test_20260703_collect_contract_diffs_reads_trait_impl_method_name",
            &format!("diffs={diffs:?}\n"),
        );

        assert!(diffs.is_empty());
    }

    #[test]
    fn test_20260703_collect_local_contract_diffs_populates_expected_fields() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::parse"), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = analysis
            .diffs
            .first()
            .expect("expected one local contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_local_contract_diffs_populates_expected_fields",
            &format!(
                "diff={diff:?}\ninferred={:?}\n",
                analysis.rvs_inferred().get(&DefPath::from("demo::parse"))
            ),
        );

        assert_eq!(diff.expected_name.rvs_as_str(), "rvs_parse");
        assert_eq!(
            analysis.rvs_inferred().get(&DefPath::from("demo::parse")),
            Some(&CapabilitySet::rvs_new())
        );
    }

    #[test]
    fn test_20260703_enforced_contract_diffs_skip_synthetic_nodes() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_generated_BI"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &rvs_make_capsmap(&[("demo::rvs_generated_BI", "BI")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diffs = &analysis.diffs;
        let has_synthetic_diff = diffs
            .iter()
            .any(|diff| diff.def_path.rvs_as_str() == "demo::rvs_generated_BI");
        let synthetic_path = DefPath::from("demo::rvs_generated_BI");
        let synthetic = analysis.rvs_synthetic_paths().contains(&synthetic_path);
        rvs_snapshot_BIS(
            "test_20260703_enforced_contract_diffs_skip_synthetic_nodes",
            &format!("synthetic={}\ndiffs={diffs:?}\n", synthetic,),
        );

        assert!(synthetic);
        assert!(graph.rvs_get("demo::rvs_generated_BI").is_none());
        assert!(!has_synthetic_diff);
    }

    #[test]
    fn test_20260806_world_port_contract_preserves_async_signature() {
        let node = FnNode {
            facts: CapabilityFacts {
                has_async: true,
                is_port_method: true,
                ..CapabilityFacts::default()
            },
            sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                "src/lib.rs".into(),
                1,
                2,
            )]),
            ..FnNode::default()
        };
        let diff = rvs_collect_single_local_contract_diff_M(
            DefPath::from("demo::rvs_fetch_P"),
            node,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        rvs_snapshot_BIS(
            "test_20260806_world_port_contract_preserves_async_signature",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(
            diff.expected_public_caps,
            CapabilitySet::rvs_from_validated("AP")
        );
        assert_eq!(
            diff.declared_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("P"))
        );
    }

    #[test]
    fn test_20260703_collect_signature_contract_diff_from_facts() {
        let diff = rvs_collect_signature_contract_diff_from_facts_M(
            DefPath::from("demo::rvs_fetch_P"),
            CapabilityFacts {
                has_async: true,
                is_port_method: true,
                ..CapabilityFacts::default()
            },
            &BTreeSet::from([CrateName::from("demo")]),
        );
        rvs_snapshot_BIS(
            "test_20260703_collect_signature_contract_diff_from_facts",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(
            diff.expected_public_caps,
            CapabilitySet::rvs_from_validated("AP")
        );
        assert_eq!(
            diff.declared_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("P"))
        );
    }

    #[test]
    fn test_20260703_summarize_contract_mismatches() {
        let diffs = vec![
            FnContractDiff {
                def_path: DefPath::from("demo::parse"),
                actual_name: FnName::from("parse"),
                expected_name: FnName::from("rvs_parse"),
                declared_public_caps: None,
                expected_public_caps: CapabilitySet::rvs_new(),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::rvs_fetch_BI"),
                actual_name: FnName::from("rvs_fetch_BI"),
                expected_name: FnName::from("rvs_fetch_P"),
                declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
                expected_public_caps: CapabilitySet::rvs_from_validated("AP"),
            },
        ];
        let items = rvs_collect_contract_mismatch_items(&diffs);
        let counts = rvs_summarize_contract_mismatch_items(&items);
        rvs_snapshot_BIS(
            "test_20260703_summarize_contract_mismatches",
            &format!("counts={counts:?}\n"),
        );

        assert_eq!(
            counts.get(&FnContractMismatchKind::MissingRvsPrefix),
            Some(&1)
        );
        assert_eq!(counts.get(&FnContractMismatchKind::NameMismatch), Some(&1));
        assert_eq!(counts.get(&FnContractMismatchKind::MissingPort), Some(&1));
    }

    #[test]
    fn test_20260703_collect_contract_mismatch_items() {
        let diffs = vec![FnContractDiff {
            def_path: DefPath::from("demo::rvs_fetch_BI"),
            actual_name: FnName::from("rvs_fetch_BI"),
            expected_name: FnName::from("rvs_fetch_P"),
            declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
            expected_public_caps: CapabilitySet::rvs_from_validated("AP"),
        }];
        let items = rvs_collect_contract_mismatch_items(&diffs);
        rvs_snapshot_BIS(
            "test_20260703_collect_contract_mismatch_items",
            &format!("items={items:?}\n"),
        );

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, FnContractMismatchKind::NameMismatch);
        assert_eq!(items[1].kind, FnContractMismatchKind::MissingAsync);
        assert_eq!(items[2].kind, FnContractMismatchKind::MissingPort);
    }

    #[test]
    fn test_20260703_contract_diff_missing_rvs_prefix() {
        let diff = FnContractDiff {
            def_path: DefPath::from("demo::parse"),
            actual_name: FnName::from("parse"),
            expected_name: FnName::from("rvs_parse"),
            declared_public_caps: None,
            expected_public_caps: CapabilitySet::rvs_new(),
        };
        rvs_snapshot_BIS(
            "test_20260703_contract_diff_missing_rvs_prefix",
            &format!("missing={}\n", diff.rvs_missing_rvs_prefix()),
        );

        assert!(diff.rvs_missing_rvs_prefix());
    }

    #[test]
    fn test_20260703_contract_diff_mismatch_kinds() {
        let missing_all = FnContractDiff {
            def_path: DefPath::from("demo::rvs_fetch"),
            actual_name: FnName::from("rvs_fetch"),
            expected_name: FnName::from("rvs_fetch_ABIMPSTU"),
            declared_public_caps: Some(CapabilitySet::rvs_new()),
            expected_public_caps: CapabilitySet::rvs_from_validated("ABIMPSTU"),
        };
        let declared_all = FnContractDiff {
            actual_name: FnName::from("rvs_fetch_ABIMPSTU"),
            declared_public_caps: Some(missing_all.expected_public_caps.clone()),
            ..missing_all.clone()
        };
        let mismatches = missing_all.rvs_mismatch_kinds();
        let no_mismatches = declared_all.rvs_mismatch_kinds();
        rvs_snapshot_BIS(
            "test_20260703_contract_diff_mismatch_kinds",
            &format!("mismatches={mismatches:?}\ndeclared_all={no_mismatches:?}\n"),
        );

        assert_eq!(
            mismatches,
            vec![
                FnContractMismatchKind::NameMismatch,
                FnContractMismatchKind::MissingAsync,
                FnContractMismatchKind::MissingBlocking,
                FnContractMismatchKind::MissingIo,
                FnContractMismatchKind::MissingMutable,
                FnContractMismatchKind::MissingPort,
                FnContractMismatchKind::MissingSideEffect,
                FnContractMismatchKind::MissingThreadLocal,
                FnContractMismatchKind::MissingUnsafe,
            ]
        );
        assert!(no_mismatches.is_empty());
        assert_eq!(
            FnContractMismatchKind::MissingAsync.rvs_as_str(),
            "missing_async"
        );
    }

    #[test]
    fn test_20260703_graph_impl_wrapper_helpers() {
        let mut graph = FnGraph::rvs_new();

        let mut impl_a = rvs_make_behavior();
        impl_a.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("demo::Reader::read@std::io::Read".into(), impl_a);

        let mut impl_b = rvs_make_behavior();
        impl_b.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("demo::Buffer::read@std::io::Read".into(), impl_b);

        let inferred = rvs_infer_caps(
            &graph,
            &rvs_make_capsmap(&[("std::fs::read_to_string", "BI")]),
        );
        let impl_index = rvs_build_impl_index(&graph);
        let alias = DefPath::from("std::io::Read::read");
        let resolved = rvs_resolve_impl_majority_caps(&alias, &impl_index, &inferred, &graph)
            .expect("graph wrapper should resolve majority caps");
        let aliases = rvs_generate_trait_aliases(&inferred, &impl_index, &graph);

        rvs_snapshot_BIS(
            "test_20260703_graph_impl_wrapper_helpers",
            &format!("resolved={resolved:?}\naliases={aliases:?}\n"),
        );

        assert!(resolved.rvs_contains(Capability::B));
        assert!(resolved.rvs_contains(Capability::I));
        assert!(aliases.contains_key(&alias));
    }

    #[test]
    fn test_20260715_trait_alias_capsmap_persists_vote_evidence() {
        let mut graph = FnGraph::rvs_new();
        for implementation in ["dep::A", "dep::B", "dep::Env"] {
            let mut node = rvs_make_behavior();
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@dep::FromString")),
                node,
            );
        }
        let inferred = BTreeMap::from([
            (
                DefPath::from("dep::A::rvs_parse@dep::FromString"),
                CapabilitySet::rvs_new(),
            ),
            (
                DefPath::from("dep::B::rvs_parse@dep::FromString"),
                CapabilitySet::rvs_new(),
            ),
            (
                DefPath::from("dep::Env::rvs_parse@dep::FromString"),
                CapabilitySet::rvs_from_validated("S"),
            ),
        ]);
        let infos = rvs_generate_trait_alias_infos(
            &inferred,
            &rvs_build_impl_index(&graph),
            &graph,
            &BTreeSet::new(),
        );
        let rendered = rvs_format_def_path_capability_info(&infos);
        let parsed = capsmap::CapsMap::rvs_parse(&rendered).unwrap();
        let info = parsed
            .rvs_lookup_info("dep::FromString::rvs_parse")
            .unwrap();
        let output = format!("rendered={rendered}basis={:?}\n", info.rvs_basis(),);
        rvs_snapshot_BIS(
            "test_20260715_trait_alias_capsmap_persists_vote_evidence",
            &output,
        );

        assert!(info.rvs_caps().rvs_is_empty());
        assert!(matches!(
            info.rvs_basis(),
            crate::capability::CapabilityBasis::TraitVote {
                implementations: 3,
                threshold: 2,
                ..
            }
        ));
    }

    #[test]
    fn test_20260715_trait_alias_capsmap_preserves_incomplete_vote_knowledge() {
        let mut graph = FnGraph::rvs_new();
        let implementation_paths: Vec<_> = ["dep::A", "dep::B"]
            .into_iter()
            .map(|implementation| {
                let path = DefPath::from(format!("{implementation}::rvs_parse@dep::FromString"));
                let mut node = rvs_make_behavior();
                node.is_trait_impl = true;
                graph.rvs_insert_M(path.clone(), node);
                path
            })
            .collect();
        let inferred = BTreeMap::from([
            (implementation_paths[0].clone(), CapabilitySet::rvs_new()),
            (
                implementation_paths[1].clone(),
                CapabilitySet::rvs_from_validated("S"),
            ),
        ]);
        let infos = rvs_generate_trait_alias_infos(
            &inferred,
            &rvs_build_impl_index(&graph),
            &graph,
            &BTreeSet::from([implementation_paths[1].clone()]),
        );
        let info = infos
            .get(&DefPath::from("dep::FromString::rvs_parse"))
            .expect("never: trait implementation vote produces an alias");
        let output = format!(
            "caps={}\ncompleteness={:?}\n",
            info.rvs_caps().rvs_letters(),
            info.rvs_completeness(),
        );
        rvs_snapshot_BIS(
            "test_20260715_trait_alias_capsmap_preserves_incomplete_vote_knowledge",
            &output,
        );

        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Incomplete);
    }

    #[test]
    fn test_20260703_collect_graph_external_dep_wrappers() {
        let mut graph = FnGraph::rvs_new();
        let mut local = rvs_make_behavior();
        local.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::fs::write"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("demo::rvs_run".into(), local);

        let local_prefixes = BTreeSet::from([CrateName::from("demo")]);
        let seed = rvs_make_capsmap(&[("std::fs::write", "BI")]);
        let inferred = rvs_infer_caps(&graph, &seed);
        let impl_index = rvs_build_impl_index(&graph);
        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &seed,
            &inferred,
            &impl_index,
        );

        rvs_snapshot_BIS(
            "test_20260703_collect_graph_external_dep_wrappers",
            &format!("known={known:?}\nunknown={unknown:?}\n"),
        );

        assert!(known.is_empty());
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260613_seed_freeze_prevents_propagation() {
        let mut graph = FnGraph::rvs_new();

        let mut cap_overflow = rvs_make_behavior();
        cap_overflow.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("core::panicking::panic"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("alloc::raw_vec::capacity_overflow".into(), cap_overflow);

        let panic = rvs_make_behavior();
        graph.rvs_insert_M("core::panicking::panic".into(), panic);

        let mut handle_error = rvs_make_behavior();
        handle_error.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("alloc::raw_vec::capacity_overflow"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("alloc::raw_vec::handle_error".into(), handle_error);

        let seed = rvs_make_capsmap(&[
            ("alloc::raw_vec::capacity_overflow", ""),
            ("alloc::raw_vec::handle_error", ""),
        ]);

        let result = rvs_infer_caps(&graph, &seed);

        let cap_caps = result.get("alloc::raw_vec::capacity_overflow");
        assert!(
            cap_caps.is_none_or(|c| c.rvs_is_empty()),
            "capacity_overflow should be frozen to empty by seed, got: {cap_caps:?}"
        );

        let handle_caps = result.get("alloc::raw_vec::handle_error");
        assert!(
            handle_caps.is_none_or(|c| c.rvs_is_empty()),
            "handle_error should be frozen to empty by seed, got: {handle_caps:?}"
        );
    }

    #[test]
    fn test_20260709_infer_caps_small_cases_table() {
        let static_ref = {
            let mut behavior = rvs_make_behavior();
            behavior.facts.has_static_ref = true;
            behavior
        };
        let suffix_name = {
            let mut behavior = rvs_make_behavior();
            behavior.facts.has_async = true;
            behavior.facts.has_mut_param = true;
            behavior
        };

        let cases = [
            (
                "single_pure",
                vec![("my_crate::rvs_add", rvs_make_behavior())],
                &[] as &[(&str, &str)],
                "my_crate::rvs_add",
                "",
            ),
            (
                "single_panic",
                vec![("my_crate::rvs_divide", rvs_make_behavior())],
                &[],
                "my_crate::rvs_divide",
                "",
            ),
            (
                "single_static_ref",
                vec![("my_crate::rvs_get_env_S", static_ref)],
                &[],
                "my_crate::rvs_get_env_S",
                "S",
            ),
            (
                "single_unsafe_block",
                vec![("my_crate::rvs_ffi_call", rvs_make_behavior())],
                &[],
                "my_crate::rvs_ffi_call",
                "",
            ),
            (
                "seed_override",
                vec![("my_crate::rvs_read_BI", rvs_make_behavior())],
                &[("my_crate::rvs_read_BI", "BI")],
                "my_crate::rvs_read_BI",
                "BI",
            ),
            (
                "suffix_from_name",
                vec![("my_crate::rvs_write_db_ABM", suffix_name)],
                &[],
                "my_crate::rvs_write_db_ABM",
                "AM",
            ),
        ];

        let mut output = String::new();
        for (name, entries, seed, key, expected_caps) in cases {
            let result = rvs_infer_caps_case_M(&entries, seed);
            let caps = result.get(key).expect("case result should contain target");
            output.push_str(&format!("{name}: {}\n", rvs_caps_to_string(caps)));
            assert_eq!(rvs_caps_to_string(caps), expected_caps, "{name}");
        }
        rvs_snapshot_BIS("test_20260709_infer_caps_small_cases_table", &output);
    }

    #[test]
    fn test_20260709_infer_caps_propagation_cycle_table() {
        let caller_gets_io = {
            let mut caller_behavior = rvs_make_behavior();
            caller_behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 2,
                    def_path: crate::symbols::DefPath::from("std::fs::read_to_string"),
                },
                CallEdgeType::Strong,
            );
            vec![
                ("my_crate::rvs_process", caller_behavior),
                ("std::fs::read_to_string", rvs_make_behavior()),
            ]
        };
        let propagation_chain = {
            let mut a_behavior = rvs_make_behavior();
            a_behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: crate::symbols::DefPath::from("my_crate::B"),
                },
                CallEdgeType::Strong,
            );
            let mut b_behavior = rvs_make_behavior();
            b_behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: crate::symbols::DefPath::from("my_crate::C"),
                },
                CallEdgeType::Strong,
            );
            vec![
                ("my_crate::A", a_behavior),
                ("my_crate::B", b_behavior),
                ("my_crate::C", rvs_make_behavior()),
            ]
        };
        let cycle_self = {
            let mut behavior = rvs_make_behavior();
            behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: crate::symbols::DefPath::from("my_crate::rvs_loop"),
                },
                CallEdgeType::Strong,
            );
            vec![("my_crate::rvs_loop", behavior)]
        };
        let cycle_mutual = {
            let mut a_behavior = rvs_make_behavior();
            a_behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: crate::symbols::DefPath::from("my_crate::B"),
                },
                CallEdgeType::Strong,
            );
            let mut b_behavior = rvs_make_behavior();
            b_behavior.calls.insert(
                FunctionIdentity {
                    crate_id: 1,
                    def_path: crate::symbols::DefPath::from("my_crate::A"),
                },
                CallEdgeType::Strong,
            );
            vec![("my_crate::A", a_behavior), ("my_crate::B", b_behavior)]
        };

        let cases = [
            (
                "caller_gets_io",
                caller_gets_io,
                &[("std::fs::read_to_string", "BI")] as &[(&str, &str)],
                vec![("my_crate::rvs_process", "BI")],
            ),
            (
                "propagation_chain",
                propagation_chain,
                &[("my_crate::C", "S")],
                vec![("my_crate::A", "S"), ("my_crate::B", "S")],
            ),
            (
                "cycle_self",
                cycle_self,
                &[],
                vec![("my_crate::rvs_loop", "")],
            ),
            (
                "cycle_mutual",
                cycle_mutual,
                &[],
                vec![("my_crate::A", ""), ("my_crate::B", "")],
            ),
        ];

        let mut output = String::new();
        for (name, entries, seed, expected_pairs) in cases {
            let result = rvs_infer_caps_case_M(&entries, seed);
            output.push_str(&format!(
                "{name}: {}\n",
                rvs_format_capsmap(&result).trim_end()
            ));
            for (key, expected_caps) in expected_pairs {
                let caps = result.get(key).expect("case result should contain key");
                assert_eq!(rvs_caps_to_string(caps), expected_caps, "{name}:{key}");
            }
        }
        rvs_snapshot_BIS("test_20260709_infer_caps_propagation_cycle_table", &output);
    }

    #[test]
    fn test_20260613_infer_caps_propagation_from_bimps_callee() {
        let mut graph = FnGraph::rvs_new();

        let mut caller_behavior = rvs_make_behavior();
        caller_behavior.facts.has_mut_param = true;
        caller_behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from(
                    "std::sys::process::unix::unix::impl::spawn",
                ),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("std::process::impl::spawn".into(), caller_behavior);

        let mut callee_behavior = rvs_make_behavior();
        callee_behavior.facts.has_mut_param = true;
        callee_behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from(
                    "std::sys::pal::unix::kernel_copy::rvs_write",
                ),
            },
            CallEdgeType::Strong,
        );
        callee_behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::sys::cycle_a"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            "std::sys::process::unix::unix::impl::spawn".into(),
            callee_behavior,
        );

        let mut cycle_a = rvs_make_behavior();
        cycle_a.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::sys::cycle_b"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("std::sys::cycle_a".into(), cycle_a);

        let mut cycle_b = rvs_make_behavior();
        cycle_b.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::sys::cycle_a"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("std::sys::cycle_b".into(), cycle_b);

        let seed = rvs_make_capsmap(&[("std::sys::pal::unix::kernel_copy::rvs_write", "BIS")]);

        let result = rvs_infer_caps(&graph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS(
            "test_20260613_infer_caps_propagation_from_bimps_callee",
            &output,
        );

        let callee_caps = result
            .get("std::sys::process::unix::unix::impl::spawn")
            .expect("callee should have entry");
        assert!(
            callee_caps.rvs_contains(Capability::B),
            "callee should have B from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::I),
            "callee should have I from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::M),
            "callee should have M from has_mut_param"
        );
        assert!(
            callee_caps.rvs_contains(Capability::S),
            "callee should have S from deep callee"
        );

        let caller_caps = result
            .get("std::process::impl::spawn")
            .expect("caller should have entry");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "caller should have B propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::I),
            "caller should have I propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::M),
            "caller should have M from has_mut_param"
        );
        assert!(
            caller_caps.rvs_contains(Capability::S),
            "caller should have S propagated from callee"
        );
    }

    // ─── rvs_resolve_impl_majority_caps ──────────────────────────────────

    #[test]
    fn test_20260613_impl_majority_vote_filters_minority_caps() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::io::Read::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::rvs_copy".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.facts.has_mut_param = true;
        file_read.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("libc::unix::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("std::fs::read@std::io::Read".into(), file_read);

        let mut cursor_read = rvs_make_behavior();
        cursor_read.facts.has_mut_param = true;
        graph.rvs_insert_M("std::io::cursor::read@std::io::Read".into(), cursor_read);

        let mut slice_read = rvs_make_behavior();
        slice_read.facts.has_mut_param = true;
        graph.rvs_insert_M("std::io::impls::read@std::io::Read".into(), slice_read);

        let seed = rvs_make_capsmap(&[("libc::unix::read", "BI")]);

        let result = rvs_infer_caps(&graph, &seed);
        rvs_snapshot_BIS(
            "test_20260613_impl_majority_vote_filters_minority_caps",
            &format!("{result:?}"),
        );

        let caller_caps = result.get("my_crate::rvs_copy").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M: not propagated"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::B),
            "B: 1/3 = minority, should not propagate"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::I),
            "I: 1/3 = minority, should not propagate"
        );
    }

    #[test]
    fn test_20260715_trait_vote_preserves_counts_and_local_outlier() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::FromString::rvs_parse"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_make_behavior();
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::FromString")),
                node,
            );
        }
        let mut env_impl = rvs_make_behavior();
        env_impl.is_trait_impl = true;
        env_impl.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::env::var"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::EnvValue::rvs_parse@demo::FromString"),
            env_impl,
        );
        let seed = rvs_make_capsmap(&[("std::env::var", "S")]);

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let vote = analysis
            .rvs_trait_votes()
            .get(&DefPath::from("demo::FromString::rvs_parse"))
            .expect("trait vote should be retained");
        let outlier = analysis
            .trait_impl_outliers
            .first()
            .expect("minority side effect should be reported as an outlier");
        let info = vote.rvs_capability_info();
        let output = format!(
            "selected={}\nimplementations={}\nthreshold={}\nS_votes={}\ncomplete={}\noutlier={}\noutlier_caps={}\ninfo_basis={:?}\n",
            vote.selected_caps.rvs_letters(),
            vote.implementations.len(),
            vote.threshold,
            vote.counts.get(&Capability::S).copied().unwrap_or(0),
            vote.rvs_is_complete(),
            outlier.implementation,
            outlier.unexpected_caps.rvs_letters(),
            info.rvs_basis(),
        );
        rvs_snapshot_BIS(
            "test_20260715_trait_vote_preserves_counts_and_local_outlier",
            &output,
        );

        assert!(vote.selected_caps.rvs_is_empty());
        assert_eq!(vote.implementations.len(), 3);
        assert_eq!(vote.threshold, 2);
        assert_eq!(vote.counts.get(&Capability::S), Some(&1));
        assert_eq!(analysis.trait_impl_outliers.len(), 1);
        assert_eq!(outlier.unexpected_caps.rvs_letters(), "S");
    }

    #[test]
    fn test_20260715_incomplete_trait_vote_suppresses_outlier_feedback() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Parser::rvs_parse"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut node = rvs_make_behavior();
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                node,
            );
        }
        let mut uncertain = rvs_make_behavior();
        uncertain.is_trait_impl = true;
        uncertain.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::uncertain"),
            },
            CallEdgeType::Strong,
        );
        let uncertain_path = DefPath::from("demo::Uncertain::rvs_parse@demo::Parser");
        graph.rvs_insert_M(uncertain_path.clone(), uncertain);
        let mut seed = capsmap::CapsMap::rvs_new();
        seed.rvs_insert_info_M(
            CapsMapKey::from("dep::uncertain"),
            CapabilityInfo::rvs_new(
                CapabilitySet::rvs_from_validated("S"),
                CapabilityBasis::Inferred,
                CapabilityCompleteness::Unknown,
            ),
        );

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let vote = analysis
            .rvs_trait_votes()
            .get(&DefPath::from("demo::Parser::rvs_parse"))
            .expect("trait vote should exist");
        let output = format!(
            "complete={}\nselected={}\nS_votes={}\nuncertain_incomplete={}\noutliers={}\n",
            vote.rvs_is_complete(),
            vote.selected_caps.rvs_letters(),
            vote.counts.get(&Capability::S).copied().unwrap_or(0),
            analysis.rvs_incomplete_paths().contains(&uncertain_path),
            analysis.trait_impl_outliers.len(),
        );
        rvs_snapshot_BIS(
            "test_20260715_incomplete_trait_vote_suppresses_outlier_feedback",
            &output,
        );

        assert!(!vote.rvs_is_complete());
        assert!(vote.selected_caps.rvs_is_empty());
        assert_eq!(vote.counts.get(&Capability::S), Some(&1));
        assert!(analysis.rvs_incomplete_paths().contains(&uncertain_path));
        assert!(analysis.trait_impl_outliers.is_empty());
    }

    #[test]
    fn test_20260715_trait_outlier_ignores_port_external_and_sourceless_impls() {
        let mut graph = FnGraph::rvs_new();

        let mut port_declaration = rvs_make_behavior();
        port_declaration.has_body = false;
        port_declaration.facts.is_port_method = true;
        graph.rvs_insert_M(
            DefPath::from("demo::ConfigClient::rvs_load_P"),
            port_declaration,
        );
        let mut port_impl = rvs_make_behavior();
        port_impl.is_trait_impl = true;
        port_impl.facts.is_port_method = true;
        port_impl.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::effect"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::EnvClient::rvs_load_P@demo::ConfigClient"),
            port_impl,
        );

        let dep_node = || {
            let mut node = rvs_make_behavior();
            node.crate_id = 2;
            node.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
            node
        };
        graph.rvs_insert_M(
            DefPath::from("dependency::Parser::rvs_parse"),
            FnNode {
                has_body: false,
                ..dep_node()
            },
        );
        for (implementation, effectful) in [
            ("dependency::A", false),
            ("dependency::B", false),
            ("dependency::Env", true),
        ] {
            let mut node = dep_node();
            node.is_trait_impl = true;
            if effectful {
                node.calls.insert(
                    FunctionIdentity {
                        crate_id: 2,
                        def_path: DefPath::from("dep::effect"),
                    },
                    CallEdgeType::Strong,
                );
            }
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@dependency::Parser")),
                node,
            );
        }

        graph.rvs_insert_M(
            DefPath::from("demo::LocalParser::rvs_parse"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        for implementation in ["demo::LocalA", "demo::LocalB"] {
            let mut node = rvs_make_behavior();
            node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::LocalParser")),
                node,
            );
        }
        let mut sourceless = FnNode {
            is_trait_impl: true,
            ..FnNode::default()
        };
        sourceless.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::effect"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Generated::rvs_parse@demo::LocalParser"),
            sourceless,
        );

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &rvs_make_capsmap(&[("dep::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = format!(
            "votes={}\noutliers={}\n",
            analysis.rvs_trait_votes().len(),
            analysis.trait_impl_outliers.len(),
        );
        rvs_snapshot_BIS(
            "test_20260715_trait_outlier_ignores_port_external_and_sourceless_impls",
            &output,
        );

        assert!(analysis.trait_impl_outliers.is_empty());
    }

    #[test]
    fn test_20260614_m_not_propagated_from_direct_call() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.facts.has_async = true;
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: crate::symbols::DefPath::from("my_crate::sort_inplace"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::handle".into(), caller);

        let mut callee = rvs_make_behavior();
        callee.facts.has_mut_param = true;
        graph.rvs_insert_M("my_crate::sort_inplace".into(), callee);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps(&graph, &seed);

        let caller_caps = result.get("my_crate::handle").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M should NOT propagate — signature-only capability"
        );
        assert!(caller_caps.rvs_contains(Capability::A), "A from has_async");
    }

    #[test]
    fn test_20260613_impl_majority_vote_no_cross_trait() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::io::Read::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::rvs_read_data".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("libc::unix::read"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("std::fs::read@std::io::Read".into(), file_read);

        let mut rwlock_read = rvs_make_behavior();
        rwlock_read.facts.has_mut_param = true;
        graph.rvs_insert_M(
            "std::sync::rwlock::read@std::sync::RwLock".into(),
            rwlock_read,
        );

        let seed = rvs_make_capsmap(&[("libc::unix::read", "BI")]);
        let result = rvs_infer_caps(&graph, &seed);
        rvs_snapshot_BIS(
            "test_20260613_impl_majority_vote_no_cross_trait",
            &format!("{result:?}"),
        );

        let caller_caps = result
            .get("my_crate::rvs_read_data")
            .expect("caller exists");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "should get B from Read::read impl"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "should NOT get M from RwLock::read (different trait)"
        );
    }

    // ─── rvs_format_capsmap ────────────────────────────────────────────

    #[test]
    fn test_20260609_format_capsmap_empty() {
        let map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_empty", &output);
        assert_eq!(output, crate::capsmap::CAPS_V2_HEADER.to_string() + "\n");
    }

    #[test]
    fn test_20260609_format_capsmap_single_entry() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_single_entry", &output);
        let parsed = capsmap::CapsMap::rvs_parse(&output).unwrap();
        assert_eq!(
            parsed.rvs_lookup("std::fs::read").unwrap().rvs_letters(),
            "BI"
        );
    }

    #[test]
    fn test_20260609_format_capsmap_multiple_sorted() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::process::exit".into(),
            CapabilitySet::rvs_from_validated("S"),
        );
        map.insert("HashMap::new".into(), CapabilitySet::rvs_new());
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_multiple_sorted", &output);
        let lines: Vec<&str> = output.trim_end().lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], crate::capsmap::CAPS_V2_HEADER);
        assert!(lines[1].contains("HashMap::new"));
        assert!(lines[2].contains("std::fs::read"));
        assert!(lines[3].contains("std::process::exit"));
    }

    #[test]
    fn test_20260715_format_def_path_capsmap_unions_specialized_impl_caps() {
        let map = BTreeMap::from([
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c75383e}::rvs_run"),
                CapabilitySet::rvs_from_validated("B"),
            ),
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c7531363e}::rvs_run"),
                CapabilitySet::rvs_from_validated("I"),
            ),
        ]);
        let output = rvs_format_def_path_capsmap(&map);
        rvs_snapshot_BIS(
            "test_20260715_format_def_path_capsmap_unions_specialized_impl_caps",
            &output,
        );

        let parsed = capsmap::CapsMap::rvs_parse(&output).unwrap();
        assert_eq!(
            parsed
                .rvs_lookup("dep::Worker::rvs_run")
                .unwrap()
                .rvs_letters(),
            "BI"
        );
    }

    #[test]
    fn test_20260716_specialized_caps_union_preserves_incomplete_knowledge() {
        let infos = BTreeMap::from([
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c7531363e}::rvs_run"),
                CapabilityInfo::rvs_inferred(CapabilitySet::rvs_from_validated("I")),
            ),
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c75383e}::rvs_run"),
                CapabilityInfo::rvs_new(
                    CapabilitySet::rvs_from_validated("B"),
                    crate::capability::CapabilityBasis::Inferred,
                    CapabilityCompleteness::Incomplete,
                ),
            ),
        ]);

        let rendered = rvs_format_def_path_capability_info(&infos);
        let parsed = capsmap::CapsMap::rvs_parse(&rendered).unwrap();
        let info = parsed
            .rvs_lookup_info("dep::Worker::rvs_run")
            .expect("never: normalized specialization is present");
        let output = format!(
            "caps={}\ncompleteness={}\n",
            info.rvs_caps().rvs_letters(),
            info.rvs_completeness().rvs_name(),
        );
        rvs_snapshot_BIS(
            "test_20260716_specialized_caps_union_preserves_incomplete_knowledge",
            &output,
        );

        assert_eq!(info.rvs_caps().rvs_letters(), "BI");
        assert_eq!(info.rvs_completeness(), CapabilityCompleteness::Incomplete);
    }

    #[test]
    fn test_20260715_format_unknown_callees_groups_specialized_impls() {
        let unknown = BTreeMap::from([
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c75383e}::run"),
                BTreeSet::from([DefPath::from("demo::rvs_call_u8")]),
            ),
            (
                DefPath::from("dep::Worker{impl#6465703a3a576f726b65723c7531363e}::run"),
                BTreeSet::from([DefPath::from("demo::rvs_call_u16")]),
            ),
        ]);
        let output = rvs_format_unknown_callees(&unknown, "unknown:\n");
        rvs_snapshot_BIS(
            "test_20260715_format_unknown_callees_groups_specialized_impls",
            &output,
        );

        assert_eq!(output.matches("dep::Worker::run=").count(), 1);
        assert!(!output.contains("{impl#"));
    }

    // ─── rvs_collect_direct_external_deps ────────────────────────────────

    #[test]
    fn test_20260630_collect_direct_external_deps_uses_bin_prefix() {
        let mut graph = FnGraph::rvs_new();
        let mut local = rvs_make_behavior();
        local.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("serde_json::de::from_str"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("cargo_rivus::rvs_parse".into(), local);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
        inferred.insert("serde_json::de::from_str".into(), CapabilitySet::rvs_new());
        let prefixes = BTreeSet::from([
            CrateName::from("rivus_linter"),
            CrateName::from("cargo_rivus"),
        ]);

        let (known, unknown) =
            rvs_collect_direct_external_deps(&graph, &prefixes, &seed, &inferred, &HashMap::new());

        rvs_snapshot_BIS(
            "test_20260630_collect_direct_external_deps_uses_bin_prefix",
            &format!("known={known:?}\nunknown={unknown:?}"),
        );
        assert!(known.contains_key("serde_json::de::from_str"));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260713_collect_direct_external_deps_includes_entry_calls() {
        let mut graph = FnGraph::rvs_new();
        let mut local = rvs_make_behavior();
        local.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("external_crate::shutdown_S"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("demo::main".into(), local);
        let inferred = BTreeMap::from([(
            DefPath::from("external_crate::shutdown_S"),
            CapabilitySet::rvs_from_validated("S"),
        )]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &BTreeSet::from([CrateName::from("demo")]),
            &capsmap::CapsMap::rvs_new(),
            &inferred,
            &HashMap::new(),
        );
        rvs_snapshot_BIS(
            "test_20260713_collect_direct_external_deps_includes_entry_calls",
            &format!("known={known:?}\nunknown={unknown:?}\n"),
        );

        assert!(known.contains_key("external_crate::shutdown_S"));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_unknown_callee_reported_as_error() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("some_external_crate::unknown_fn"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from([CrateName::from("my_crate")]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(known.is_empty());
        assert!(
            unknown.contains_key("some_external_crate::unknown_fn"),
            "unknown callee must be reported as error"
        );
        assert_eq!(unknown.len(), 1);
        assert!(unknown["some_external_crate::unknown_fn"].contains("my_crate::caller"));
    }

    #[test]
    fn test_20260611_inferred_callee_is_known() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("some_external_crate::known_fn"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
        inferred.insert(
            "some_external_crate::known_fn".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let local_prefixes = BTreeSet::from([CrateName::from("my_crate")]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        let caps = known
            .get("some_external_crate::known_fn")
            .expect("should have entry in known");
        assert!(caps.rvs_caps().rvs_contains(Capability::B));
        assert!(caps.rvs_caps().rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260704_collect_direct_external_deps_uses_resolver_for_bodyless_decl() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("dep::Fetcher::rvs_fetch_BI"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("my_crate::rvs_run"), caller);
        graph.rvs_insert_M(
            DefPath::from("dep::Fetcher::rvs_fetch_BI"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );
        let inferred = BTreeMap::from([(
            DefPath::from("dep::Fetcher::rvs_fetch_BI"),
            CapabilitySet::rvs_new(),
        )]);
        let local_prefixes = BTreeSet::from([CrateName::from("my_crate")]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &capsmap::CapsMap::rvs_new(),
            &inferred,
            &rvs_build_impl_index(&graph),
        );
        let caps = known
            .get("dep::Fetcher::rvs_fetch_BI")
            .expect("external declaration should be known from declared suffix");
        let output = format!(
            "caps={}\nunknown={unknown:?}\n",
            rvs_caps_to_string(caps.rvs_caps()),
        );
        rvs_snapshot_BIS(
            "test_20260704_collect_direct_external_deps_uses_resolver_for_bodyless_decl",
            &output,
        );

        assert!(caps.rvs_caps().rvs_contains(Capability::B));
        assert!(caps.rvs_caps().rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_seed_callee_is_skipped() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::fs::write"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::caller".into(), behavior);

        let seed = rvs_make_capsmap(&[("std::fs::write", "BI")]);
        let inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from([CrateName::from("my_crate")]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(!known.contains_key("std::fs::write"));
        assert!(!unknown.contains_key("std::fs::write"));
    }

    #[test]
    fn test_20260613_inherent_impl_no_collision() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::time::SystemTime::now"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M("my_crate::rvs_get_time".into(), behavior);

        let seed = rvs_make_capsmap(&[("std::time::SystemTime::now", "S")]);

        let inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from([CrateName::from("my_crate")]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(
            !unknown.contains_key("std::time::SystemTime::now"),
            "seed entry should match the full def_path"
        );
        assert!(!known.contains_key("std::time::SystemTime::now"));
    }

    // ─── coverage ────────────────────────────────────────────────────────

    #[test]
    fn test_20260630_main_helper_coverage() {
        let mut merged = rvs_make_behavior();
        let mut other = rvs_make_behavior();
        other.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: crate::symbols::DefPath::from("std::io::Read::read"),
            },
            CallEdgeType::Strong,
        );
        other.facts.has_async = true;
        merged.rvs_merge_M(&other);
        assert!(
            merged
                .calls
                .keys()
                .any(|k| k.def_path == "std::io::Read::read".into())
        );
        assert!(merged.facts.has_async);

        let mut graph = FnGraph::rvs_new();
        let mut impl_behavior = rvs_make_behavior();
        impl_behavior.facts.has_mut_param = true;
        let inferred_caps = rvs_infer_signature_caps(&impl_behavior);
        graph.rvs_insert_M("std::fs::read@std::io::Read".into(), impl_behavior);

        let impl_index = rvs_build_impl_index(&graph);
        assert!(impl_index.contains_key("read@std::io::Read"));

        assert!(inferred_caps.rvs_contains(Capability::M));

        let mut unknown = BTreeMap::new();
        unknown.insert(
            DefPath::from("missing::fn"),
            BTreeSet::from([DefPath::from("caller::fn")]),
        );
        let formatted = rvs_format_unknown_callees(&unknown, "header\n");
        assert!(formatted.contains("missing::fn"));

        let caps_str = rvs_caps_to_string(&CapabilitySet::rvs_from_validated("BI"));
        assert_eq!(caps_str, "BI");

        let inferred = BTreeMap::from([(
            DefPath::from("std::fs::read@std::io::Read"),
            CapabilitySet::rvs_from_validated("BI"),
        )]);
        let aliases = rvs_generate_trait_aliases(&inferred, &impl_index, &graph);
        assert_eq!(
            aliases.get("std::io::Read::read"),
            Some(&CapabilitySet::rvs_from_validated("BI"))
        );

        let majority = rvs_resolve_impl_majority_caps(
            &DefPath::from("std::io::Read::read"),
            &impl_index,
            &inferred,
            &graph,
        );
        assert_eq!(majority, Some(CapabilitySet::rvs_from_validated("BI")));
    }

    #[test]
    fn test_20260710_port_scope_keeps_local_and_clears_external() {
        let mut local = rvs_make_behavior();
        local.facts.is_port_method = true;
        let mut dependency = rvs_make_behavior();
        dependency.facts.is_port_method = true;
        dependency.crate_id = 2;
        dependency.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        let mut std_method = rvs_make_behavior();
        std_method.facts.is_port_method = true;
        std_method.crate_id = 3;
        std_method.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("app::ApiClient::rvs_fetch_P"), local);
        graph.rvs_insert_M(DefPath::from("dependency::HttpClient::fetch"), dependency);
        graph.rvs_insert_M(DefPath::from("std::internal::CacheClient::get"), std_method);

        rvs_scope_port_methods_M(&mut graph, &BTreeSet::from([CrateName::from("app")]));

        let mut output = String::new();
        for (path, node) in graph.rvs_iter() {
            output.push_str(&format!("{path}: port={}\n", node.facts.is_port_method));
        }
        rvs_snapshot_BIS(
            "test_20260710_port_scope_keeps_local_and_clears_external",
            &output,
        );
        assert!(
            graph
                .rvs_get("app::ApiClient::rvs_fetch_P")
                .is_some_and(|node| node.facts.is_port_method)
        );
        assert!(
            graph
                .rvs_get("dependency::HttpClient::fetch")
                .is_some_and(|node| !node.facts.is_port_method)
        );
        assert!(
            graph
                .rvs_get("std::internal::CacheClient::get")
                .is_some_and(|node| !node.facts.is_port_method)
        );
    }

    #[test]
    fn test_20260716_port_scope_clears_external_target_facts() {
        let port_facts = CapabilityFacts {
            is_port_method: true,
            ..CapabilityFacts::default()
        };
        let mut local = rvs_make_behavior();
        local.facts = port_facts;
        local.facts = port_facts;
        let mut external = rvs_make_behavior();
        external.facts = port_facts;
        let external_target = external.rvs_test_target_M(20);
        external_target.facts = port_facts;
        external_target.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("app::ApiClient::rvs_fetch_P"), local);
        graph.rvs_insert_M(DefPath::from("dependency::HttpClient::fetch"), external);

        rvs_scope_port_methods_M(&mut graph, &BTreeSet::from([CrateName::from("app")]));

        let local_target_port = graph
            .rvs_get("app::ApiClient::rvs_fetch_P")
            .map(|node| node)
            .is_some_and(|target| target.facts.is_port_method);
        let external_target_port = graph
            .rvs_get("dependency::HttpClient::fetch")
            .map(|node| node)
            .is_some_and(|target| target.facts.is_port_method);
        let output = format!(
            "local_target_port={local_target_port}\nexternal_target_port={external_target_port}\n"
        );
        rvs_snapshot_BIS(
            "test_20260716_port_scope_clears_external_target_facts",
            &output,
        );

        assert!(local_target_port);
        assert!(!external_target_port);
    }

    #[test]
    fn test_20260710_external_port_fact_uses_capsmap_after_scoping() {
        let external_path = DefPath::from("dependency::HttpClient::fetch");
        let mut external = rvs_make_behavior();
        external.facts.is_port_method = true;
        external.crate_id = 2;
        external.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        let mut caller = rvs_make_behavior();
        caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: external_path.clone(),
            },
            CallEdgeType::Strong,
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("app::rvs_run_BI"), caller);
        graph.rvs_insert_M(external_path.clone(), external);
        let seed = rvs_make_capsmap(&[("dependency::HttpClient::fetch", "BI")]);

        rvs_scope_port_methods_M(&mut graph, &BTreeSet::from([CrateName::from("app")]));
        let inferred = rvs_infer_caps(&graph, &seed);
        let external_caps = inferred.get(&external_path).unwrap();
        let caller_caps = inferred.get(&DefPath::from("app::rvs_run_BI")).unwrap();
        let output = format!(
            "external={}\ncaller={}\n",
            rvs_caps_to_string(external_caps),
            rvs_caps_to_string(caller_caps),
        );
        rvs_snapshot_BIS(
            "test_20260710_external_port_fact_uses_capsmap_after_scoping",
            &output,
        );

        assert_eq!(
            Some(external_caps),
            Some(&CapabilitySet::rvs_from_str("BI").unwrap())
        );
        assert_eq!(
            Some(caller_caps),
            Some(&CapabilitySet::rvs_from_str("BI").unwrap())
        );
    }

    #[test]
    fn test_20260710_initial_caps_precedence_table() {
        let mut port = rvs_make_behavior();
        port.facts.is_port_method = true;
        port.facts.has_async = true;
        let mut seeded = rvs_make_behavior();
        seeded.facts.has_async = true;
        let mut signature = rvs_make_behavior();
        signature.facts.has_async = true;
        signature.facts.has_mut_param = true;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("app::ApiClient::fetch"), port);
        graph.rvs_insert_M(DefPath::from("app::rvs_seeded_S"), seeded);
        graph.rvs_insert_M(DefPath::from("app::rvs_signature_AM"), signature);
        let seed = rvs_make_capsmap(&[("app::ApiClient::fetch", "BI"), ("app::rvs_seeded_S", "S")]);

        let initial = rvs_initial_caps(&graph, &seed);
        let mut output = initial
            .iter()
            .map(|(path, caps)| format!("{path}={}", rvs_caps_to_string(caps)))
            .collect::<Vec<_>>()
            .join("\n");
        output.push('\n');
        rvs_snapshot_BIS("test_20260710_initial_caps_precedence_table", &output);

        assert_eq!(initial.len(), 3);
    }

    #[test]
    fn test_20260809_dependency_body_with_thread_local_rng_infers_BIST() {
        let mut graph = FnGraph::rvs_new();
        let mut local_caller = rvs_make_behavior();
        local_caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("tempfile::Builder::tempdir_in"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("cargo_rivus::rvs_reserve"), local_caller);

        let mut tempdir_in = rvs_make_behavior();
        tempdir_in.has_body = true;
        tempdir_in.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("tempfile::util::create_helper"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("tempfile::Builder::tempdir_in"), tempdir_in);

        let mut create_helper = rvs_make_behavior();
        create_helper.has_body = true;
        create_helper.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("fastrand::Rng::new"),
            },
            CallEdgeType::Strong,
        );
        create_helper.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::env::current_dir"),
            },
            CallEdgeType::Strong,
        );
        create_helper.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("std::fs::create_dir"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("tempfile::util::create_helper"),
            create_helper,
        );

        let mut rng_new = rvs_make_behavior();
        rng_new.has_body = true;
        rng_new.facts.has_thread_local_ref = true;
        graph.rvs_insert_M(DefPath::from("fastrand::Rng::new"), rng_new);

        let mut current_dir = rvs_make_behavior();
        current_dir.has_body = true;
        current_dir.facts.has_static_ref = true;
        graph.rvs_insert_M(DefPath::from("std::env::current_dir"), current_dir);

        let mut create_dir = rvs_make_behavior();
        create_dir.has_body = true;
        create_dir.facts.has_static_ref = true;
        graph.rvs_insert_M(DefPath::from("std::fs::create_dir"), create_dir);

        let local_prefixes = BTreeSet::from([CrateName::from("cargo_rivus")]);
        let seed = rvs_make_capsmap(&[
            ("std::env::current_dir", "BS"),
            ("std::fs::create_dir", "BIS"),
        ]);
        let prepared = PreparedInference::rvs_prepare(&graph, &seed, &local_prefixes);
        let (known, unknown) =
            prepared.rvs_collect_direct_external_deps(&graph, &local_prefixes, &seed);

        let mut output = String::new();
        for (path, info) in &known {
            output.push_str(&format!(
                "{path} = {} (incomplete={})\n",
                info.rvs_caps().rvs_letters(),
                info.rvs_completeness() == CapabilityCompleteness::Incomplete,
            ));
        }
        output.push_str(&format!("unknown_count={}\n", unknown.len()));
        rvs_snapshot_BIS(
            "test_20260809_dependency_body_with_thread_local_rng_infers_BIST",
            &output,
        );

        let tempdir_caps = known
            .get("tempfile::Builder::tempdir_in")
            .expect("tempdir_in must be a known external dep");
        assert!(
            tempdir_caps.rvs_caps().rvs_contains(Capability::T),
            "tempdir_in calls fastrand which uses thread-local RNG; T must propagate"
        );
        assert!(
            tempdir_caps.rvs_caps().rvs_contains(Capability::B),
            "tempdir_in performs blocking file I/O; B must propagate"
        );
        assert!(
            tempdir_caps.rvs_caps().rvs_contains(Capability::I),
            "tempdir_in performs file I/O; I must propagate"
        );
        assert!(
            tempdir_caps.rvs_caps().rvs_contains(Capability::S),
            "tempdir_in reads current_dir (static ref) and fastrand (S); S must propagate"
        );
        assert!(unknown.is_empty(), "no unknown callees expected");
    }

    #[test]
    fn test_20260809_dependency_pure_constructor_infers_pure() {
        let mut graph = FnGraph::rvs_new();
        let mut local_caller = rvs_make_behavior();
        local_caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("tempfile::Builder::new"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("cargo_rivus::rvs_run"), local_caller);

        let mut dep_fn = rvs_make_behavior();
        dep_fn.has_body = true;
        graph.rvs_insert_M(DefPath::from("tempfile::Builder::new"), dep_fn);

        let local_prefixes = BTreeSet::from([CrateName::from("cargo_rivus")]);
        let prepared =
            PreparedInference::rvs_prepare(&graph, &capsmap::CapsMap::rvs_new(), &local_prefixes);
        let (known, unknown) = prepared.rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &capsmap::CapsMap::rvs_new(),
        );

        let caps = known
            .get("tempfile::Builder::new")
            .expect("pure constructor must be a known external dep");
        let output = format!(
            "caps={}\nunknown_count={}\n",
            caps.rvs_caps().rvs_letters(),
            unknown.len(),
        );
        rvs_snapshot_BIS(
            "test_20260809_dependency_pure_constructor_infers_pure",
            &output,
        );

        assert!(
            caps.rvs_caps().rvs_is_empty(),
            "pure constructor with no side effects must infer as pure"
        );
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260809_dependency_mut_self_setter_infers_M() {
        let mut graph = FnGraph::rvs_new();
        let mut local_caller = rvs_make_behavior();
        local_caller.calls.insert(
            FunctionIdentity {
                crate_id: 2,
                def_path: DefPath::from("tempfile::Builder::prefix"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("cargo_rivus::rvs_run"), local_caller);

        let mut dep_fn = rvs_make_behavior();
        dep_fn.has_body = true;
        dep_fn.facts.has_mut_param = true;
        graph.rvs_insert_M(DefPath::from("tempfile::Builder::prefix"), dep_fn);

        let local_prefixes = BTreeSet::from([CrateName::from("cargo_rivus")]);
        let prepared =
            PreparedInference::rvs_prepare(&graph, &capsmap::CapsMap::rvs_new(), &local_prefixes);
        let (known, unknown) = prepared.rvs_collect_direct_external_deps(
            &graph,
            &local_prefixes,
            &capsmap::CapsMap::rvs_new(),
        );

        let caps = known
            .get("tempfile::Builder::prefix")
            .expect("setter with &mut self must be a known external dep");
        let output = format!(
            "caps={}\nunknown_count={}\n",
            caps.rvs_caps().rvs_letters(),
            unknown.len(),
        );
        rvs_snapshot_BIS("test_20260809_dependency_mut_self_setter_infers_M", &output);

        assert!(
            caps.rvs_caps().rvs_contains(Capability::M),
            "setter with &mut self must infer M"
        );
        assert!(unknown.is_empty());
    }
}
