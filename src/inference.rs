use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::artifacts::{FnGraph, FnNode};
use crate::capability::{
    Capability, CapabilityPolicy, CapabilitySet, ParsedFunctionName, rvs_parse_function,
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
    synthetic_paths: BTreeSet<DefPath>,
    incomplete_paths: BTreeSet<DefPath>,
}

impl PreparedInference {
    pub(crate) fn rvs_prepare_M(
        graph: &mut FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        rvs_scope_port_methods_M(graph, local_crate_names);
        let impl_index = rvs_build_impl_index(graph);
        let inferred = rvs_infer_caps_with_index(graph, seed, &impl_index);
        let synthetic_paths = inferred
            .keys()
            .filter(|path| graph.rvs_get(path.rvs_as_str()).is_none())
            .cloned()
            .collect();
        let mut prepared = Self {
            inferred,
            impl_index,
            synthetic_paths,
            incomplete_paths: BTreeSet::new(),
        };
        prepared.incomplete_paths =
            rvs_incomplete_inference_paths(graph, seed, &prepared.inferred, &prepared.impl_index);
        prepared
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

    pub(crate) fn rvs_resolver<'a>(
        &'a self,
        graph: &'a FnGraph,
        seed: &'a capsmap::CapsMap,
    ) -> CalleeCapsResolver<'a> {
        CalleeCapsResolver::rvs_new(graph, seed, &self.inferred, &self.impl_index)
    }

    pub(crate) fn rvs_collect_direct_external_deps(
        &self,
        graph: &FnGraph,
        local_crate_names: &BTreeSet<CrateName>,
        seed: &capsmap::CapsMap,
    ) -> (
        BTreeMap<DefPath, CapabilitySet>,
        BTreeMap<DefPath, BTreeSet<DefPath>>,
    ) {
        let local_scope = LocalScope::rvs_new(local_crate_names);
        let mut known = BTreeMap::new();
        let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
        let resolver = self.rvs_resolver(graph, seed);
        for (func, behavior) in graph.rvs_iter() {
            if !local_scope.rvs_contains(func) {
                continue;
            }
            for callee in behavior.rvs_dependency_calls() {
                if local_scope.rvs_contains(callee) || seed.rvs_lookup_def_path(callee).is_some() {
                    continue;
                }
                if self.incomplete_paths.contains(callee) {
                    unknown
                        .entry(callee.clone())
                        .or_default()
                        .insert(func.clone());
                } else if let Some(caps) = resolver.rvs_for_propagation_target(callee) {
                    known.entry(callee.clone()).or_insert(caps);
                } else {
                    unknown
                        .entry(callee.clone())
                        .or_default()
                        .insert(func.clone());
                }
            }
        }
        (known, unknown)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedLocalAnalysis {
    pub(crate) diffs: Vec<FnContractDiff>,
    inference: PreparedInference,
}

impl PreparedLocalAnalysis {
    pub(crate) fn rvs_prepare_M(
        graph: &mut FnGraph,
        seed: &capsmap::CapsMap,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Self {
        let inference = PreparedInference::rvs_prepare_M(graph, seed, local_crate_names);
        let diffs = rvs_collect_contract_diffs_with_incomplete(
            graph,
            inference.rvs_inferred(),
            local_crate_names,
            inference.rvs_incomplete_paths(),
        );
        Self { diffs, inference }
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

    pub(crate) fn rvs_resolver<'a>(
        &'a self,
        graph: &'a FnGraph,
        seed: &'a capsmap::CapsMap,
    ) -> CalleeCapsResolver<'a> {
        self.inference.rvs_resolver(graph, seed)
    }
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

#[derive(Debug)]
pub(crate) struct CalleeCapsResolver<'a> {
    graph: &'a FnGraph,
    caps: &'a capsmap::CapsMap,
    inferred: &'a BTreeMap<DefPath, CapabilitySet>,
    impl_index: &'a HashMap<TraitMethodKey, Vec<DefPath>>,
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
            caps,
            inferred,
            impl_index,
        }
    }

    pub(crate) fn rvs_for_propagation_target(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.rvs_resolve(callee, PROPAGATION_TARGET_PRECEDENCE)
    }

    pub(crate) fn rvs_for_contract_check(&self, callee: &DefPath) -> Option<CapabilitySet> {
        self.rvs_resolve(callee, CONTRACT_CHECK_PRECEDENCE)
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
            CalleeCapsSource::PortMethod => self
                .graph
                .rvs_get(callee.rvs_as_str())
                .filter(|node| node.facts.is_port_method)
                .map(|_| CapabilityPolicy::rvs_port_method_caps()),
            CalleeCapsSource::ExactCapsMap => self.caps.rvs_lookup_def_path(callee).cloned(),
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
                        || node.facts.is_port_method
                        || rvs_declared_caps_from_def_path(callee).is_some()
                })
                .then(|| self.inferred.get(callee).cloned())
                .flatten(),
            CalleeCapsSource::DeclaredCaps => rvs_declared_caps_from_def_path(callee),
            CalleeCapsSource::ImplMajority => {
                if callee.rvs_trait_method_identity().is_some() {
                    None
                } else {
                    rvs_resolve_impl_majority_caps(
                        callee,
                        self.impl_index,
                        self.inferred,
                        self.graph,
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
        let mut caps =
            rvs_resolve_impl_majority_caps(callee, self.impl_index, self.inferred, self.graph)?;
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

pub(crate) fn rvs_infer_caps_with_index(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> BTreeMap<DefPath, CapabilitySet> {
    let mut inferred = rvs_initial_caps(graph, seed);
    for (_, behavior) in graph.rvs_iter() {
        for callee in &behavior.calls {
            if !inferred.contains_key(callee)
                && let Some(caps) = seed.rvs_lookup_def_path(callee)
            {
                inferred.insert(callee.clone(), caps.clone());
            } else if !inferred.contains_key(callee)
                && let Some(caps) = rvs_declared_caps_from_def_path(callee)
            {
                inferred.insert(callee.clone(), caps);
            }
        }
    }

    loop {
        let mut changed = false;
        for (func, behavior) in graph.rvs_iter() {
            if seed.rvs_lookup_def_path(func).is_some() {
                continue;
            }
            if behavior.facts.is_port_method {
                continue;
            }
            let mut combined = inferred
                .get(func)
                .cloned()
                .unwrap_or_else(CapabilitySet::rvs_new);
            let resolver = CalleeCapsResolver::rvs_new(graph, seed, &inferred, impl_index);
            for callee in &behavior.calls {
                let callee_caps = resolver.rvs_for_propagation_target(callee);
                if let Some(cc) = callee_caps {
                    changed |= combined.rvs_extend_filtered_M(&cc, |cap| {
                        CapabilityPolicy::rvs_is_propagated_cap(cap)
                    });
                }
            }
            inferred.insert(func.clone(), combined);
        }
        if !changed {
            break;
        }
    }
    let bodyless_paths: Vec<DefPath> = graph
        .rvs_iter()
        .filter(|(_, behavior)| !behavior.has_body)
        .map(|(path, _)| path.clone())
        .collect();
    for func in bodyless_paths {
        if seed.rvs_lookup_def_path(&func).is_some() {
            continue;
        }
        if let Some(caps) = CalleeCapsResolver::rvs_new(graph, seed, &inferred, impl_index)
            .rvs_for_propagation_target(&func)
        {
            inferred.insert(func, caps);
        }
    }
    inferred
}

pub(crate) fn rvs_initial_caps(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
) -> BTreeMap<DefPath, CapabilitySet> {
    graph
        .rvs_iter()
        .map(|(func, behavior)| {
            let caps = if behavior.facts.is_port_method {
                CapabilityPolicy::rvs_port_method_caps()
            } else if let Some(caps) = seed.rvs_lookup_def_path(func) {
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
    let scope = LocalScope::rvs_new(local_crate_names);
    for (def_path, node) in graph.rvs_iter_mut_M() {
        if node.facts.is_port_method && !scope.rvs_contains(def_path) {
            node.facts.is_port_method = false;
        }
    }
    debug_assert!(
        graph
            .rvs_iter()
            .all(|(def_path, node)| { !node.facts.is_port_method || scope.rvs_contains(def_path) })
    );
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
    let scope = LocalScope::rvs_new(local_crate_names);
    let mut diffs = Vec::new();
    for (def_path, node) in graph.rvs_iter() {
        if !FunctionClassification::rvs_new(&scope, def_path, node).rvs_is_contract_enforced() {
            continue;
        }
        let expected_public_caps = inferred
            .get(def_path)
            .expect("never: prepared inference covers every graph node");
        let actual_name = def_path.rvs_fn_name();
        let declared_public_caps =
            rvs_parse_function(actual_name.rvs_as_str()).map(|(_, caps)| caps);
        let mut naming_caps = expected_public_caps.clone();
        if incomplete_paths.contains(def_path)
            && let Some(declared_caps) = &declared_public_caps
        {
            let _ = naming_caps.rvs_extend_filtered_M(declared_caps, |capability| {
                CapabilityPolicy::rvs_is_propagated_cap(capability)
            });
        }
        let expected_name = rvs_expected_contract_name(&actual_name, &naming_caps);
        diffs.push(FnContractDiff {
            def_path: def_path.clone(),
            actual_name,
            expected_name,
            declared_public_caps,
            expected_public_caps: expected_public_caps.clone(),
        });
    }
    diffs
}

fn rvs_incomplete_inference_paths(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> BTreeSet<DefPath> {
    let resolver = CalleeCapsResolver::rvs_new(graph, seed, inferred, impl_index);
    let mut incomplete: BTreeSet<DefPath> = graph
        .rvs_iter()
        .filter(|(path, _)| !rvs_is_inference_taint_barrier(path, graph, seed))
        .filter(|(_, node)| {
            node.calls
                .iter()
                .any(|callee| resolver.rvs_for_contract_check(callee).is_none())
        })
        .map(|(path, _)| path.clone())
        .collect();

    loop {
        let mut newly_incomplete: BTreeSet<DefPath> = impl_index
            .values()
            .filter(|implementations| {
                implementations
                    .iter()
                    .any(|implementation| incomplete.contains(implementation))
            })
            .filter_map(|implementations| {
                implementations
                    .iter()
                    .find_map(|implementation| implementation.rvs_trait_method_identity())
                    .map(|identity| identity.rvs_trait_method_path())
            })
            .filter(|path| !incomplete.contains(path))
            .filter(|path| !rvs_is_inference_taint_barrier(path, graph, seed))
            .collect();
        newly_incomplete.extend(
            graph
                .rvs_iter()
                .filter(|(path, _)| !incomplete.contains(*path))
                .filter(|(path, _)| !rvs_is_inference_taint_barrier(path, graph, seed))
                .filter(|(_, node)| node.calls.iter().any(|callee| incomplete.contains(callee)))
                .map(|(path, _)| path.clone()),
        );
        if newly_incomplete.is_empty() {
            return incomplete;
        }
        incomplete.extend(newly_incomplete);
    }
}

fn rvs_is_inference_taint_barrier(
    path: &DefPath,
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
) -> bool {
    seed.rvs_lookup_def_path(path).is_some()
        || graph
            .rvs_get(path.rvs_as_str())
            .is_some_and(|node| node.facts.is_port_method)
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

/// Resolve a trait method callee by taking an at-least-half vote across all
/// impl methods for each propagated capability.
pub(crate) fn rvs_resolve_impl_majority_caps(
    callee: &DefPath,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
) -> Option<CapabilitySet> {
    let lookup_key = TraitMethodKey::rvs_from_trait_method(callee)?;
    let impl_keys = impl_index.get(&lookup_key)?;

    for key in impl_keys {
        if let Some(behavior) = graph.rvs_get(key.rvs_as_str())
            && behavior.facts.is_port_method
        {
            let mut caps = CapabilitySet::rvs_new();
            caps.rvs_insert_M(Capability::P);
            return Some(caps);
        }
    }

    let mut cap_counts: HashMap<Capability, usize> = HashMap::new();
    let mut total = 0usize;
    for key in impl_keys {
        if let Some(caps) = inferred.get(key) {
            total += 1;
            for cap in caps.rvs_iter() {
                if CapabilityPolicy::rvs_is_propagated_cap(cap) {
                    *cap_counts.entry(cap).or_default() += 1;
                }
            }
        }
    }

    if total == 0 {
        return None;
    }

    let threshold = total.div_ceil(2);
    let mut majority_caps = CapabilitySet::rvs_new();
    for (cap, count) in &cap_counts {
        if *count >= threshold {
            majority_caps.rvs_insert_M(*cap);
        }
    }
    Some(majority_caps)
}

pub(crate) fn rvs_format_capsmap<K>(caps: &BTreeMap<K, CapabilitySet>) -> String
where
    K: AsRef<str> + Ord,
{
    let mut lines: Vec<String> = caps
        .iter()
        .map(|(name, cs)| {
            let caps_str = cs.rvs_letters();
            if caps_str.is_empty() {
                format!("{}=", name.as_ref())
            } else {
                let desc = cs.rvs_descriptions();
                format!("{}={caps_str} # {desc}", name.as_ref())
            }
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

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

#[cfg(test)]
pub(crate) fn rvs_collect_direct_external_deps(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<TraitMethodKey, Vec<DefPath>>,
) -> (
    BTreeMap<DefPath, CapabilitySet>,
    BTreeMap<DefPath, BTreeSet<DefPath>>,
) {
    let prepared = PreparedInference {
        inferred: inferred.clone(),
        impl_index: impl_index.clone(),
        synthetic_paths: BTreeSet::new(),
        incomplete_paths: rvs_incomplete_inference_paths(graph, seed, inferred, impl_index),
    };
    prepared.rvs_collect_direct_external_deps(graph, local_crate_names, seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityFacts;
    use crate::test_support::rvs_snapshot_BIS;

    /// Helper: build a default `FnNode` with all flags false and no calls.
    fn rvs_make_behavior() -> FnNode {
        FnNode {
            sources: BTreeSet::from([crate::artifacts::FnSource::rvs_new(
                "src/lib.rs".into(),
                1,
                2,
            )]),
            ..FnNode::default()
        }
    }

    #[test]
    fn test_20260711_prepare_local_analysis_builds_shared_derivatives() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(DefPath::from("dep::read"));
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
        inner.calls.insert(DefPath::from("dep::unknown"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_inner_BIS"), inner);
        let mut outer = rvs_make_behavior();
        outer.calls.insert(DefPath::from("demo::rvs_inner_BIS"));
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
        behavior.calls.insert(DefPath::from("dep::unknown"));
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
        caller
            .calls
            .insert(DefPath::from("demo::ApiClient::rvs_fetch_P"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_call_PS"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let mut impl_method = rvs_make_behavior();
        impl_method.is_trait_impl = true;
        impl_method.facts.is_port_method = true;
        impl_method.calls.insert(DefPath::from("dep::unknown"));
        graph.rvs_insert_M(
            DefPath::from("demo::DiskClient::rvs_fetch_P@demo::ApiClient"),
            impl_method,
        );

        let mut seeded = rvs_make_behavior();
        seeded.calls.insert(DefPath::from("dep::unknown"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_seeded_BIS"), seeded);
        let seed = capsmap::CapsMap::rvs_parse("demo::rvs_seeded_BIS=S\n").unwrap();

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
        implementation.calls.insert(DefPath::from("dep::unknown"));
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
        caller
            .calls
            .insert(DefPath::from("dep::DavFileSystem::open"));
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
    fn test_20260715_direct_external_dep_rejects_incomplete_wrapper_caps() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(DefPath::from("dep::log"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut wrapper = rvs_make_behavior();
        wrapper.calls.insert(DefPath::from("dep::Log::log"));
        graph.rvs_insert_M(DefPath::from("dep::log"), wrapper);
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
        let (known, unknown) = rvs_collect_direct_external_deps(
            &graph,
            &local,
            &seed,
            inference.rvs_inferred(),
            inference.rvs_impl_index(),
        );
        let output = format!(
            "known={}\nunknown={}\ncaller_recorded={}\n",
            known.contains_key("dep::log"),
            unknown.contains_key("dep::log"),
            unknown
                .get("dep::log")
                .is_some_and(|callers| callers.contains("demo::rvs_run")),
        );
        rvs_snapshot_BIS(
            "test_20260715_direct_external_dep_rejects_incomplete_wrapper_caps",
            &output,
        );

        assert!(!known.contains_key("dep::log"));
        assert!(unknown.contains_key("dep::log"));
    }

    #[test]
    fn test_20260715_direct_external_trait_dispatch_rejects_incomplete_impl() {
        let mut graph = FnGraph::rvs_new();
        let dispatch_path = DefPath::from("dep::Fetcher::rvs_fetch_BI");

        let mut caller = rvs_make_behavior();
        caller.calls.insert(dispatch_path.clone());
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_BI"), caller);
        graph.rvs_insert_M(
            dispatch_path.clone(),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let mut implementation = rvs_make_behavior();
        implementation.is_trait_impl = true;
        implementation.calls.insert(DefPath::from("dep::unknown"));
        graph.rvs_insert_M(
            DefPath::from("dep::MemoryFetcher::rvs_fetch_BI@dep::Fetcher"),
            implementation,
        );

        let local = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_new();
        let inference = PreparedInference::rvs_prepare_M(&mut graph, &seed, &local);
        let (known, unknown) = inference.rvs_collect_direct_external_deps(&graph, &local, &seed);
        let output = format!(
            "dispatch_incomplete={}\nknown={}\nunknown={}\ncaller_recorded={}\n",
            inference.rvs_incomplete_paths().contains(&dispatch_path),
            known.contains_key(&dispatch_path),
            unknown.contains_key(&dispatch_path),
            unknown
                .get(&dispatch_path)
                .is_some_and(|callers| callers.contains("demo::rvs_run_BI")),
        );
        rvs_snapshot_BIS(
            "test_20260715_direct_external_trait_dispatch_rejects_incomplete_impl",
            &output,
        );

        assert!(inference.rvs_incomplete_paths().contains(&dispatch_path));
        assert!(!known.contains_key(&dispatch_path));
        assert!(unknown.contains_key(&dispatch_path));
    }

    #[test]
    fn test_20260712_prepare_inference_builds_shared_derivatives_once() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(DefPath::from("dep::read"));
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
            &BTreeSet::from([DefPath::from("dep::read")])
        );
    }

    fn rvs_infer_caps_case_M(
        entries: &[(&str, FnNode)],
        seed_text: &str,
    ) -> BTreeMap<DefPath, CapabilitySet> {
        let mut graph = FnGraph::rvs_new();
        for (path, behavior) in entries {
            graph.rvs_insert_M(DefPath::from(*path), behavior.clone());
        }
        let seed = capsmap::CapsMap::rvs_parse(seed_text).unwrap();
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
            node.calls.insert(callee);
            graph.rvs_insert_M(DefPath::from(format!("demo::rvs_f{i:02}")), node);
        }
        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();

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
        node.calls.insert(DefPath::from("dep::rvs_write_BI"));
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
        caller.calls.insert(DefPath::from("dep::rvs_send_AEIS"));
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
        caller
            .calls
            .insert(DefPath::from("demo::Fetcher::rvs_fetch"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch"),
            FnNode {
                has_body: false,
                ..rvs_make_behavior()
            },
        );

        let mut impl_method = rvs_make_behavior();
        impl_method
            .calls
            .insert(DefPath::from("std::fs::read_to_string"));
        graph.rvs_insert_M(
            DefPath::from("demo::DiskFetcher::rvs_fetch@demo::Fetcher"),
            impl_method,
        );

        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();
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
        caller
            .calls
            .insert(DefPath::from("demo::Fetcher::rvs_fetch_BI"));
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
        caller
            .calls
            .insert(DefPath::from("demo::Fetcher::rvs_fetch_A"));
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
        caller
            .calls
            .insert(DefPath::from("demo::ApiClient::rvs_fetch_P"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let mut impl_method = rvs_make_behavior();
        impl_method
            .calls
            .insert(DefPath::from("std::fs::read_to_string"));
        graph.rvs_insert_M(
            DefPath::from("demo::DiskClient::rvs_fetch_P@demo::ApiClient"),
            impl_method,
        );

        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();
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
        caller
            .calls
            .insert(DefPath::from("demo::ApiClient::rvs_fetch_P"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);

        let mut trait_decl = rvs_make_behavior();
        trait_decl.has_body = false;
        trait_decl.facts.is_port_method = true;
        graph.rvs_insert_M(DefPath::from("demo::ApiClient::rvs_fetch_P"), trait_decl);

        let seed = capsmap::CapsMap::rvs_parse("demo::ApiClient::rvs_fetch_P=BI\n").unwrap();
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

        let caps =
            capsmap::CapsMap::rvs_parse("demo::ApiClient::rvs_fetch_P=BI\ndemo::rvs_exact_S=BI\n")
                .unwrap();
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
                propagation: Some("P"),
                contract: Some("P"),
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
        run.calls.insert(DefPath::from("std::fs::read_to_string"));
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), run);
        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();

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
        run.calls.insert(DefPath::from("std::fs::read_to_string"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), run);
        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();
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
        caller.calls.insert(DefPath::from("demo::rvs_generated_BI"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &capsmap::CapsMap::rvs_parse("demo::rvs_generated_BI=BI").unwrap(),
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
    fn test_20260703_collect_single_local_contract_diff_port_ignores_async() {
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
            "test_20260703_collect_single_local_contract_diff_port_ignores_async",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(
            diff.expected_public_caps,
            CapabilitySet::rvs_from_validated("P")
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
            CapabilitySet::rvs_from_validated("P")
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
        impl_a.calls.insert("std::fs::read_to_string".into());
        graph.rvs_insert_M("demo::Reader::read@std::io::Read".into(), impl_a);

        let mut impl_b = rvs_make_behavior();
        impl_b.calls.insert("std::fs::read_to_string".into());
        graph.rvs_insert_M("demo::Buffer::read@std::io::Read".into(), impl_b);

        let inferred = rvs_infer_caps(
            &graph,
            &capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap(),
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
    fn test_20260703_collect_graph_external_dep_wrappers() {
        let mut graph = FnGraph::rvs_new();
        let mut local = rvs_make_behavior();
        local.calls.insert("std::fs::write".into());
        graph.rvs_insert_M("demo::rvs_run".into(), local);

        let local_prefixes = BTreeSet::from([CrateName::from("demo")]);
        let seed = capsmap::CapsMap::rvs_parse("std::fs::write=BI").unwrap();
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
        cap_overflow.calls.insert("core::panicking::panic".into());
        graph.rvs_insert_M("alloc::raw_vec::capacity_overflow".into(), cap_overflow);

        let panic = rvs_make_behavior();
        graph.rvs_insert_M("core::panicking::panic".into(), panic);

        let mut handle_error = rvs_make_behavior();
        handle_error
            .calls
            .insert("alloc::raw_vec::capacity_overflow".into());
        graph.rvs_insert_M("alloc::raw_vec::handle_error".into(), handle_error);

        let seed = capsmap::CapsMap::rvs_parse(
            "alloc::raw_vec::capacity_overflow=\nalloc::raw_vec::handle_error=\n",
        )
        .unwrap();

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
                "",
                "my_crate::rvs_add",
                "",
            ),
            (
                "single_panic",
                vec![("my_crate::rvs_divide", rvs_make_behavior())],
                "",
                "my_crate::rvs_divide",
                "",
            ),
            (
                "single_static_ref",
                vec![("my_crate::rvs_get_env_S", static_ref)],
                "",
                "my_crate::rvs_get_env_S",
                "S",
            ),
            (
                "single_unsafe_block",
                vec![("my_crate::rvs_ffi_call", rvs_make_behavior())],
                "",
                "my_crate::rvs_ffi_call",
                "",
            ),
            (
                "seed_override",
                vec![("my_crate::rvs_read_BI", rvs_make_behavior())],
                "my_crate::rvs_read_BI=BI",
                "my_crate::rvs_read_BI",
                "BI",
            ),
            (
                "suffix_from_name",
                vec![("my_crate::rvs_write_db_ABM", suffix_name)],
                "",
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
            caller_behavior
                .calls
                .insert("std::fs::read_to_string".into());
            vec![
                ("my_crate::rvs_process", caller_behavior),
                ("std::fs::read_to_string", rvs_make_behavior()),
            ]
        };
        let propagation_chain = {
            let mut a_behavior = rvs_make_behavior();
            a_behavior.calls.insert("my_crate::B".into());
            let mut b_behavior = rvs_make_behavior();
            b_behavior.calls.insert("my_crate::C".into());
            vec![
                ("my_crate::A", a_behavior),
                ("my_crate::B", b_behavior),
                ("my_crate::C", rvs_make_behavior()),
            ]
        };
        let cycle_self = {
            let mut behavior = rvs_make_behavior();
            behavior.calls.insert("my_crate::rvs_loop".into());
            vec![("my_crate::rvs_loop", behavior)]
        };
        let cycle_mutual = {
            let mut a_behavior = rvs_make_behavior();
            a_behavior.calls.insert("my_crate::B".into());
            let mut b_behavior = rvs_make_behavior();
            b_behavior.calls.insert("my_crate::A".into());
            vec![("my_crate::A", a_behavior), ("my_crate::B", b_behavior)]
        };

        let cases = [
            (
                "caller_gets_io",
                caller_gets_io,
                "std::fs::read_to_string=BI",
                vec![("my_crate::rvs_process", "BI")],
            ),
            (
                "propagation_chain",
                propagation_chain,
                "my_crate::C=S",
                vec![("my_crate::A", "S"), ("my_crate::B", "S")],
            ),
            (
                "cycle_self",
                cycle_self,
                "",
                vec![("my_crate::rvs_loop", "")],
            ),
            (
                "cycle_mutual",
                cycle_mutual,
                "",
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
        caller_behavior
            .calls
            .insert("std::sys::process::unix::unix::impl::spawn".into());
        graph.rvs_insert_M("std::process::impl::spawn".into(), caller_behavior);

        let mut callee_behavior = rvs_make_behavior();
        callee_behavior.facts.has_mut_param = true;
        callee_behavior
            .calls
            .insert("std::sys::pal::unix::kernel_copy::rvs_write".into());
        callee_behavior.calls.insert("std::sys::cycle_a".into());
        graph.rvs_insert_M(
            "std::sys::process::unix::unix::impl::spawn".into(),
            callee_behavior,
        );

        let mut cycle_a = rvs_make_behavior();
        cycle_a.calls.insert("std::sys::cycle_b".into());
        graph.rvs_insert_M("std::sys::cycle_a".into(), cycle_a);

        let mut cycle_b = rvs_make_behavior();
        cycle_b.calls.insert("std::sys::cycle_a".into());
        graph.rvs_insert_M("std::sys::cycle_b".into(), cycle_b);

        let seed =
            capsmap::CapsMap::rvs_parse("std::sys::pal::unix::kernel_copy::rvs_write=BIS").unwrap();

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
        caller.calls.insert("std::io::Read::read".into());
        graph.rvs_insert_M("my_crate::rvs_copy".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.facts.has_mut_param = true;
        file_read.calls.insert("libc::unix::read".into());
        graph.rvs_insert_M("std::fs::read@std::io::Read".into(), file_read);

        let mut cursor_read = rvs_make_behavior();
        cursor_read.facts.has_mut_param = true;
        graph.rvs_insert_M("std::io::cursor::read@std::io::Read".into(), cursor_read);

        let mut slice_read = rvs_make_behavior();
        slice_read.facts.has_mut_param = true;
        graph.rvs_insert_M("std::io::impls::read@std::io::Read".into(), slice_read);

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();

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
    fn test_20260614_m_not_propagated_from_direct_call() {
        let mut graph = FnGraph::rvs_new();

        let mut caller = rvs_make_behavior();
        caller.facts.has_async = true;
        caller.calls.insert("my_crate::sort_inplace".into());
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
        caller.calls.insert("std::io::Read::read".into());
        graph.rvs_insert_M("my_crate::rvs_read_data".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.calls.insert("libc::unix::read".into());
        graph.rvs_insert_M("std::fs::read@std::io::Read".into(), file_read);

        let mut rwlock_read = rvs_make_behavior();
        rwlock_read.facts.has_mut_param = true;
        graph.rvs_insert_M(
            "std::sync::rwlock::read@std::sync::RwLock".into(),
            rwlock_read,
        );

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();
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
        assert_eq!(output, "\n");
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
        assert_eq!(output, "std::fs::read=BI # Blocking IO\n");
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
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("HashMap::new"));
        assert!(lines[1].starts_with("std::fs::read"));
        assert!(lines[2].starts_with("std::process::exit"));
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

        assert_eq!(output, "dep::Worker::rvs_run=BI # Blocking IO\n");
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
        local.calls.insert("serde_json::de::from_str".into());
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
        local
            .entry_calls
            .insert("external_crate::shutdown_S".into());
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
        behavior
            .calls
            .insert("some_external_crate::unknown_fn".into());
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
        behavior
            .calls
            .insert("some_external_crate::known_fn".into());
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
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260704_collect_direct_external_deps_uses_resolver_for_bodyless_decl() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller
            .calls
            .insert(DefPath::from("dep::Fetcher::rvs_fetch_BI"));
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
        let output = format!("caps={}\nunknown={unknown:?}\n", rvs_caps_to_string(caps),);
        rvs_snapshot_BIS(
            "test_20260704_collect_direct_external_deps_uses_resolver_for_bodyless_decl",
            &output,
        );

        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_seed_callee_is_skipped() {
        let mut graph = FnGraph::rvs_new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::fs::write".into());
        graph.rvs_insert_M("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("std::fs::write=BI").unwrap();
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
        behavior.calls.insert("std::time::SystemTime::now".into());
        graph.rvs_insert_M("my_crate::rvs_get_time".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("std::time::SystemTime::now=S").unwrap();

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
        other.calls.insert("std::io::Read::read".into());
        other.facts.has_async = true;
        merged.rvs_merge_M(&other);
        assert!(merged.calls.contains("std::io::Read::read"));
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
        let mut std_method = rvs_make_behavior();
        std_method.facts.is_port_method = true;
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
    fn test_20260710_external_port_fact_uses_capsmap_after_scoping() {
        let external_path = DefPath::from("dependency::HttpClient::fetch");
        let mut external = rvs_make_behavior();
        external.facts.is_port_method = true;
        let mut caller = rvs_make_behavior();
        caller.calls.insert(external_path.clone());
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("app::rvs_run_BI"), caller);
        graph.rvs_insert_M(external_path.clone(), external);
        let seed = capsmap::CapsMap::rvs_parse("dependency::HttpClient::fetch=BI").unwrap();

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
        let seed =
            capsmap::CapsMap::rvs_parse("app::ApiClient::fetch=BI\napp::rvs_seeded_S=S").unwrap();

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
}
