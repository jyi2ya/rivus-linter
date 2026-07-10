use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::artifacts::{FnGraph, FnNode};
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet};
use crate::capability::{rvs_extract_raw_suffix, rvs_parse_function};
use crate::capsmap;
use crate::symbols::{CrateName, DefPath, FnName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FnContractDiff {
    pub(crate) def_path: DefPath,
    pub(crate) actual_name: FnName,
    pub(crate) expected_name: Option<FnName>,
    pub(crate) declared_public_caps: Option<CapabilitySet>,
    pub(crate) expected_public_caps: Option<CapabilitySet>,
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
        self.expected_name
            .as_ref()
            .is_some_and(|expected| expected != &self.actual_name)
    }

    pub(crate) fn rvs_missing_rvs_prefix(&self) -> bool {
        self.expected_name.is_some() && !self.actual_name.rvs_as_str().starts_with("rvs_")
    }

    pub(crate) fn rvs_mismatch_kinds(&self) -> Vec<FnContractMismatchKind> {
        let mut mismatches = Vec::new();
        if self.rvs_missing_rvs_prefix() {
            mismatches.push(FnContractMismatchKind::MissingRvsPrefix);
        } else if self.rvs_has_name_mismatch() {
            mismatches.push(FnContractMismatchKind::NameMismatch);
        }
        if let Some(expected_caps) = self.expected_public_caps.as_ref() {
            let declared_has = |cap| {
                self.declared_public_caps
                    .as_ref()
                    .is_some_and(|caps| caps.rvs_contains(cap))
            };
            if expected_caps.rvs_contains(Capability::A) && !declared_has(Capability::A) {
                mismatches.push(FnContractMismatchKind::MissingAsync);
            }
            if expected_caps.rvs_contains(Capability::B) && !declared_has(Capability::B) {
                mismatches.push(FnContractMismatchKind::MissingBlocking);
            }
            if expected_caps.rvs_contains(Capability::I) && !declared_has(Capability::I) {
                mismatches.push(FnContractMismatchKind::MissingIo);
            }
            if expected_caps.rvs_contains(Capability::M) && !declared_has(Capability::M) {
                mismatches.push(FnContractMismatchKind::MissingMutable);
            }
            if expected_caps.rvs_contains(Capability::P) && !declared_has(Capability::P) {
                mismatches.push(FnContractMismatchKind::MissingPort);
            }
            if expected_caps.rvs_contains(Capability::S) && !declared_has(Capability::S) {
                mismatches.push(FnContractMismatchKind::MissingSideEffect);
            }
            if expected_caps.rvs_contains(Capability::T) && !declared_has(Capability::T) {
                mismatches.push(FnContractMismatchKind::MissingThreadLocal);
            }
            if expected_caps.rvs_contains(Capability::U) && !declared_has(Capability::U) {
                mismatches.push(FnContractMismatchKind::MissingUnsafe);
            }
        }
        mismatches
    }
}

pub(crate) fn rvs_make_callee_display(def_path: &str, src_path: Option<&str>) -> String {
    if let Some(sp) = src_path {
        if sp != def_path {
            format!("{sp} ({def_path})")
        } else {
            def_path.to_string()
        }
    } else {
        def_path.to_string()
    }
}

pub(crate) fn rvs_collect_call_contract_mismatch(
    def_path: &str,
    src_path: Option<&str>,
    caps: &CapabilitySet,
    callee_caps: Option<&CapabilitySet>,
) -> Option<CallContractMismatch> {
    let callee_display = rvs_make_callee_display(def_path, src_path);
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
pub(crate) fn rvs_build_impl_index(graph: &FnGraph) -> HashMap<String, Vec<DefPath>> {
    let mut idx: HashMap<String, Vec<DefPath>> = HashMap::new();
    for key in graph.rvs_keys() {
        if let Some(at_pos) = key.rvs_as_str().find('@') {
            let (method, suffix_with_sep) = key.rvs_as_str().split_at(at_pos);
            let Some(suffix) = suffix_with_sep.strip_prefix('@') else {
                continue;
            };
            let method_name = DefPath::from(method).rvs_fn_name();
            let lookup = format!("{method_name}@{suffix}");
            idx.entry(lookup).or_default().push(key.clone());
        }
    }
    idx
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
    let mut msg = String::from(header);
    for (callee, callers) in unknown {
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
    impl_index: &HashMap<String, Vec<DefPath>>,
    graph: &FnGraph,
) -> BTreeMap<DefPath, CapabilitySet> {
    let mut aliases = BTreeMap::new();
    let mut seen = HashSet::new();
    for key in inferred.keys() {
        if let Some(at_pos) = key.rvs_as_str().find('@') {
            let (method_full, trait_path_with_sep) = key.rvs_as_str().split_at(at_pos);
            let Some(trait_path) = trait_path_with_sep.strip_prefix('@') else {
                continue;
            };
            if let Some(method_name) = method_full.rsplit("::").next() {
                let alias = DefPath::rvs_new(format!("{trait_path}::{method_name}"));
                if seen.insert(alias.clone())
                    && let Some(voted) =
                        rvs_resolve_impl_majority_caps(&alias, impl_index, inferred, graph)
                {
                    aliases.insert(alias, voted);
                }
            }
        }
    }
    aliases
}

/// Convert a `CapabilitySet` to its uppercase letter string.
pub(crate) fn rvs_caps_to_string(caps: &CapabilitySet) -> String {
    caps.rvs_iter().map(|c| c.rvs_as_char()).collect()
}

pub(crate) fn rvs_infer_caps(
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
) -> BTreeMap<DefPath, CapabilitySet> {
    let mut inferred: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();

    for (func, behavior) in graph.rvs_iter() {
        if behavior.facts.is_port_method {
            inferred.insert(func.clone(), CapabilityPolicy::rvs_port_method_caps());
        } else if let Some(caps) = seed.rvs_lookup(func.rvs_as_str()) {
            inferred.insert(func.clone(), caps.clone());
        } else {
            inferred.insert(func.clone(), rvs_infer_signature_caps(behavior));
        }
    }
    for (_, behavior) in graph.rvs_iter() {
        for callee in &behavior.calls {
            if !inferred.contains_key(callee)
                && let Some(caps) = seed.rvs_lookup(callee.rvs_as_str())
            {
                inferred.insert(callee.clone(), caps.clone());
            } else if !inferred.contains_key(callee)
                && let Some(caps) = rvs_declared_caps_from_def_path(callee)
            {
                inferred.insert(callee.clone(), caps);
            }
        }
    }

    let impl_index = rvs_build_impl_index(graph);

    loop {
        let mut changed = false;
        for (func, behavior) in graph.rvs_iter() {
            if seed.rvs_lookup(func.rvs_as_str()).is_some() {
                continue;
            }
            if behavior.facts.is_port_method {
                continue;
            }
            let mut combined = inferred
                .get(func)
                .cloned()
                .unwrap_or_else(CapabilitySet::rvs_new);
            for callee in &behavior.calls {
                let callee_caps =
                    rvs_resolve_callee_caps(callee, graph, seed, &inferred, &impl_index);
                if let Some(cc) = callee_caps {
                    for cap in cc.rvs_iter() {
                        if !CapabilityPolicy::rvs_is_propagated_cap(cap) {
                            continue;
                        }
                        if !combined.rvs_contains(cap) {
                            combined.rvs_insert_M(cap);
                            changed = true;
                        }
                    }
                }
            }
            inferred.insert(func.clone(), combined);
        }
        if !changed {
            break;
        }
    }
    let impl_index = rvs_build_impl_index(graph);
    let bodyless_paths: Vec<DefPath> = graph
        .rvs_iter()
        .filter(|(_, behavior)| !behavior.has_body)
        .map(|(path, _)| path.clone())
        .collect();
    for func in bodyless_paths {
        if seed.rvs_lookup(func.rvs_as_str()).is_some() {
            continue;
        }
        if let Some(caps) = rvs_resolve_callee_caps(&func, graph, seed, &inferred, &impl_index) {
            inferred.insert(func, caps);
        }
    }
    inferred
}

fn rvs_resolve_callee_caps(
    callee: &DefPath,
    graph: &FnGraph,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<String, Vec<DefPath>>,
) -> Option<CapabilitySet> {
    if graph
        .rvs_get(callee.rvs_as_str())
        .is_some_and(|node| node.facts.is_port_method)
    {
        return Some(CapabilityPolicy::rvs_port_method_caps());
    }
    if let Some(caps) = seed.rvs_lookup(callee.rvs_as_str()) {
        return Some(caps.clone());
    }
    if !callee.rvs_as_str().contains('@') {
        let is_bodyless_decl = graph
            .rvs_get(callee.rvs_as_str())
            .is_some_and(|node| !node.has_body);
        if is_bodyless_decl
            && let Some(mut caps) =
                rvs_resolve_impl_majority_caps(callee, impl_index, inferred, graph)
        {
            if let Some(signature_caps) = inferred.get(callee) {
                for cap in signature_caps.rvs_iter() {
                    if !CapabilityPolicy::rvs_is_propagated_cap(cap) {
                        caps.rvs_insert_M(cap);
                    }
                }
            }
            return Some(caps);
        }
    }
    let declared_caps = rvs_declared_caps_from_def_path(callee);
    if graph
        .rvs_get(callee.rvs_as_str())
        .is_some_and(|node| !node.has_body)
        && declared_caps.is_some()
    {
        return declared_caps;
    }

    inferred.get(callee).cloned().or(declared_caps).or_else(|| {
        if !callee.rvs_as_str().contains('@') {
            rvs_resolve_impl_majority_caps(callee, impl_index, inferred, graph)
        } else {
            None
        }
    })
}

fn rvs_declared_caps_from_def_path(def_path: &DefPath) -> Option<CapabilitySet> {
    let fn_name = def_path.rvs_fn_name();
    let raw_suffix = rvs_extract_raw_suffix(fn_name.rvs_as_str());
    let has_unknown_suffix = raw_suffix
        .chars()
        .any(|letter| Capability::rvs_from_char(letter).is_none());
    let caps = rvs_parse_function(fn_name.rvs_as_str()).map(|(_, caps)| caps)?;
    if has_unknown_suffix && caps.rvs_is_empty() {
        return None;
    }
    Some(caps)
}

pub(crate) fn rvs_infer_graph_M(graph: &mut FnGraph, seed: &capsmap::CapsMap) {
    graph.nodes.retain(|_, node| !node.is_synthetic);
    let inferred = rvs_infer_caps(graph, seed);
    for (_, node) in graph.rvs_iter_mut_M() {
        node.rvs_clear_expected_public_caps_M();
    }
    for (func, caps) in inferred {
        if let Some(node) = graph.rvs_get_mut_M(&func) {
            node.rvs_set_expected_public_caps_M(caps);
        } else {
            let node = FnNode {
                is_synthetic: true,
                has_body: false,
                expected_public_caps: Some(caps),
                ..FnNode::default()
            };
            graph.rvs_insert_M(func, node);
        }
    }
}

pub(crate) fn rvs_project_expected_local_names_M(
    graph: &mut FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) {
    let local_prefixes: Vec<_> = local_crate_names
        .iter()
        .map(CrateName::rvs_prefix)
        .collect();
    let root_main_paths: BTreeSet<DefPath> = local_prefixes
        .iter()
        .map(|prefix| prefix.rvs_join_name(&FnName::rvs_new("main")))
        .collect();

    for (full_path, node) in graph.rvs_iter_mut_M() {
        node.rvs_clear_expected_name_M();
        let Some(caps) = node.expected_public_caps.as_ref() else {
            continue;
        };
        let Some(relative_path) = local_prefixes
            .iter()
            .find_map(|prefix| full_path.rvs_strip_prefix(prefix))
        else {
            continue;
        };
        let short_name = relative_path.rvs_fn_name();
        if root_main_paths.contains(full_path)
            || node.is_test
            || node.is_trait_impl
            || node.is_synthetic
        {
            continue;
        }
        let caps_str = rvs_caps_to_string(caps);
        let base_name = rvs_contract_base_name(short_name.rvs_as_str(), &caps_str);
        let expected_name = if caps_str.is_empty() {
            FnName::rvs_new(format!("rvs_{base_name}"))
        } else {
            FnName::rvs_new(format!("rvs_{base_name}_{caps_str}"))
        };
        node.rvs_set_expected_name_M(expected_name);
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

pub(crate) fn rvs_contract_diff_is_enforced(
    graph: &FnGraph,
    diff: &FnContractDiff,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    let root_main_paths: BTreeSet<_> = local_crate_names
        .iter()
        .map(|name| name.rvs_prefix().rvs_join_name(&FnName::rvs_new("main")))
        .collect();
    if root_main_paths.contains(&diff.def_path) {
        return false;
    }
    let Some(node) = graph.rvs_get(diff.def_path.rvs_as_str()) else {
        return false;
    };
    !node.is_test && !node.is_trait_impl && !node.is_synthetic
}

pub(crate) fn rvs_collect_enforced_contract_diffs(
    graph: &FnGraph,
    diffs: &[FnContractDiff],
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    diffs
        .iter()
        .filter(|diff| rvs_contract_diff_is_enforced(graph, diff, local_crate_names))
        .cloned()
        .collect()
}

pub(crate) fn rvs_collect_contract_diffs(
    graph: &FnGraph,
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    let local_prefixes: Vec<_> = local_crate_names
        .iter()
        .map(CrateName::rvs_prefix)
        .collect();
    let mut diffs = Vec::new();
    for (def_path, node) in graph.rvs_iter() {
        if !local_prefixes
            .iter()
            .any(|prefix| def_path.rvs_starts_with(prefix))
        {
            continue;
        }
        let actual_name = def_path.rvs_fn_name();
        let declared_public_caps =
            rvs_parse_function(actual_name.rvs_as_str()).map(|(_, caps)| caps);
        diffs.push(FnContractDiff {
            def_path: def_path.clone(),
            actual_name,
            expected_name: node.expected_name.clone(),
            declared_public_caps,
            expected_public_caps: node.expected_public_caps.clone(),
        });
    }
    diffs
}

pub(crate) fn rvs_collect_local_contract_diffs_M(
    graph: &mut FnGraph,
    seed: &capsmap::CapsMap,
    local_crate_names: &BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    rvs_infer_graph_M(graph, seed);
    rvs_project_expected_local_names_M(graph, local_crate_names);
    rvs_collect_contract_diffs(graph, local_crate_names)
}

pub(crate) fn rvs_summarize_contract_mismatches(
    diffs: &[FnContractDiff],
) -> BTreeMap<FnContractMismatchKind, usize> {
    let mut counts = BTreeMap::new();
    for mismatch in rvs_collect_contract_mismatch_items(diffs) {
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
            calls: BTreeSet::new(),
            facts,
            has_body: true,
            is_trait_impl: false,
            is_test: false,
            sources: BTreeSet::new(),
            report_caps: None,
            report_line_count: None,
            allows_dead_code: false,
            is_synthetic: false,
            expected_public_caps: None,
            expected_name: None,
        },
        local_crate_names,
    )
}

/// Resolve a trait method callee by taking an at-least-half vote across all
/// impl methods for each propagated capability.
pub(crate) fn rvs_resolve_impl_majority_caps(
    callee: &DefPath,
    impl_index: &HashMap<String, Vec<DefPath>>,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    graph: &FnGraph,
) -> Option<CapabilitySet> {
    let (trait_path, method) = callee.rvs_as_str().rsplit_once("::")?;
    let lookup_key = format!("{method}@{trait_path}");
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
            let caps_str = rvs_caps_to_string(cs);
            if caps_str.is_empty() {
                format!("{}=", name.as_ref())
            } else {
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{}={caps_str} # {desc}", name.as_ref())
            }
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

pub(crate) fn rvs_collect_direct_external_deps(
    graph: &FnGraph,
    local_crate_prefixes: &BTreeSet<CrateName>,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<DefPath, CapabilitySet>,
    impl_index: &HashMap<String, Vec<DefPath>>,
) -> (
    BTreeMap<DefPath, CapabilitySet>,
    BTreeMap<DefPath, BTreeSet<DefPath>>,
) {
    let local_prefixes: Vec<_> = local_crate_prefixes
        .iter()
        .map(CrateName::rvs_prefix)
        .collect();
    let mut known: BTreeMap<DefPath, CapabilitySet> = BTreeMap::new();
    let mut unknown: BTreeMap<DefPath, BTreeSet<DefPath>> = BTreeMap::new();
    for (func, behavior) in graph.rvs_iter() {
        if !local_prefixes
            .iter()
            .any(|prefix| func.rvs_starts_with(prefix))
        {
            continue;
        }
        for callee in &behavior.calls {
            if local_prefixes
                .iter()
                .any(|prefix| callee.rvs_starts_with(prefix))
            {
                continue;
            }
            if seed.rvs_lookup(callee.rvs_as_str()).is_some() {
                continue;
            }
            if let Some(caps) = rvs_resolve_callee_caps(callee, graph, seed, inferred, impl_index) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityFacts;
    use crate::test_support::rvs_snapshot_BIS;

    /// Helper: build a default `FnNode` with all flags false and no calls.
    fn rvs_make_behavior() -> FnNode {
        FnNode {
            calls: BTreeSet::new(),
            facts: CapabilityFacts::default(),
            has_body: true,
            is_trait_impl: false,
            is_test: false,
            sources: BTreeSet::new(),
            report_caps: None,
            report_line_count: None,
            allows_dead_code: false,
            is_synthetic: false,
            expected_public_caps: None,
            expected_name: None,
        }
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

        let caps = rvs_resolve_callee_caps(
            &DefPath::from("demo::Fetcher::rvs_fetch"),
            &graph,
            &capsmap::CapsMap::rvs_new(),
            &inferred,
            &impl_index,
        )
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

        let caps = rvs_resolve_callee_caps(
            &DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            &graph,
            &capsmap::CapsMap::rvs_new(),
            &inferred,
            &impl_index,
        )
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
    fn test_20260703_infer_graph_sets_node_caps() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();

        rvs_infer_graph_M(&mut graph, &seed);

        let caps = graph
            .rvs_get("demo::rvs_run")
            .and_then(|node| node.expected_public_caps.clone());
        rvs_snapshot_BIS(
            "test_20260703_infer_graph_sets_node_caps",
            &format!("caps={caps:?}\n"),
        );

        assert!(caps.is_some());
    }

    #[test]
    fn test_20260704_infer_graph_prunes_stale_synthetic_nodes() {
        let mut graph = FnGraph::rvs_new();
        let mut run = rvs_make_behavior();
        run.calls.insert(DefPath::from("std::fs::read_to_string"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), run);
        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();
        rvs_infer_graph_M(&mut graph, &seed);
        assert!(graph.rvs_get("std::fs::read_to_string").is_some());

        graph
            .rvs_get_mut_M(&DefPath::from("demo::rvs_run"))
            .expect("demo node should exist")
            .calls
            .clear();
        rvs_infer_graph_M(&mut graph, &capsmap::CapsMap::rvs_new());

        let has_synthetic = graph.rvs_get("std::fs::read_to_string").is_some();
        let run_caps = graph
            .rvs_get("demo::rvs_run")
            .and_then(|node| node.expected_public_caps.clone())
            .expect("demo node should keep expected caps");
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
        let mut node = rvs_make_behavior();
        node.rvs_set_expected_public_caps_M(CapabilitySet::rvs_from_validated("BI"));
        graph.rvs_insert_M(DefPath::from("demo::parse"), node);

        rvs_project_expected_local_names_M(&mut graph, &BTreeSet::from([CrateName::from("demo")]));

        let expected_name = graph
            .rvs_get("demo::parse")
            .and_then(|node| node.expected_name.as_ref())
            .map(FnName::rvs_as_str)
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
        let mut node = rvs_make_behavior();
        node.rvs_set_expected_public_caps_M(CapabilitySet::rvs_new());
        graph.rvs_insert_M(DefPath::from("demo::parse_BI"), node);

        rvs_project_expected_local_names_M(&mut graph, &BTreeSet::from([CrateName::from("demo")]));

        let expected_name = graph
            .rvs_get("demo::parse_BI")
            .and_then(|node| node.expected_name.as_ref())
            .map(FnName::rvs_as_str)
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
            let mut node = rvs_make_behavior();
            node.rvs_set_expected_public_caps_M(CapabilitySet::rvs_new());
            graph.rvs_insert_M(DefPath::from(name), node);
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
                && diff
                    .expected_name
                    .as_ref()
                    .is_some_and(|name| name.rvs_as_str() == "rvs_Foo")
                && diff.rvs_missing_rvs_prefix()
        }));
        assert!(diffs.iter().any(|diff| {
            diff.actual_name.rvs_as_str() == "_helper"
                && diff
                    .expected_name
                    .as_ref()
                    .is_some_and(|name| name.rvs_as_str() == "rvs__helper")
                && diff.rvs_missing_rvs_prefix()
        }));
    }

    #[test]
    fn test_20260703_collect_local_contract_diffs_updates_existing_rvs_suffix() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::rvs_fetch_ABI"),
            FnNode {
                calls: BTreeSet::new(),
                facts: CapabilityFacts {
                    is_port_method: true,
                    ..CapabilityFacts::default()
                },
                has_body: true,
                is_trait_impl: false,
                is_test: false,
                sources: BTreeSet::new(),
                report_caps: None,
                report_line_count: None,
                allows_dead_code: false,
                is_synthetic: false,
                expected_public_caps: None,
                expected_name: None,
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

        assert_eq!(
            diff.expected_name.as_ref().map(FnName::rvs_as_str),
            Some("rvs_fetch_P")
        );
        assert!(diff.rvs_has_name_mismatch());
    }

    #[test]
    fn test_20260706_local_trait_decl_expected_name_uses_impl_vote() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Fetcher::rvs_fetch_BI"),
            FnNode {
                calls: BTreeSet::new(),
                facts: CapabilityFacts::default(),
                has_body: false,
                is_trait_impl: false,
                is_test: false,
                sources: BTreeSet::new(),
                report_caps: None,
                report_line_count: None,
                allows_dead_code: false,
                is_synthetic: false,
                expected_public_caps: None,
                expected_name: None,
            },
        );
        graph.rvs_insert_M(
            DefPath::from("demo::MemoryFetcher::rvs_fetch_BI@demo::Fetcher"),
            FnNode::default(),
        );

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = diffs.first().expect("expected trait declaration diff");
        rvs_snapshot_BIS(
            "test_20260706_local_trait_decl_expected_name_uses_impl_vote",
            &format!(
                "actual={}\nexpected={:?}\ncaps={:?}\n",
                diff.actual_name, diff.expected_name, diff.expected_public_caps
            ),
        );

        assert_eq!(
            diff.expected_name.as_ref().map(FnName::rvs_as_str),
            Some("rvs_fetch")
        );
        assert!(
            diff.expected_public_caps
                .as_ref()
                .is_some_and(CapabilitySet::rvs_is_empty)
        );
    }

    #[test]
    fn test_20260703_collect_contract_diffs_reports_name_and_caps_mismatch() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_make_behavior();
        node.rvs_set_expected_public_caps_M(CapabilitySet::rvs_from_validated("P"));
        node.rvs_set_expected_name_M(FnName::from("rvs_fetch_P"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_fetch_ABI"), node);

        let diffs = rvs_collect_contract_diffs(&graph, &BTreeSet::from([CrateName::from("demo")]));
        let diff = diffs.first().expect("expected one contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_contract_diffs_reports_name_and_caps_mismatch",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(diff.actual_name.rvs_as_str(), "rvs_fetch_ABI");
        assert!(diff.rvs_has_name_mismatch());
        assert_eq!(
            diff.expected_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("P"))
        );
        assert_eq!(
            diff.declared_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("ABI"))
        );
    }

    #[test]
    fn test_20260703_collect_contract_diffs_reads_trait_impl_method_name() {
        let mut graph = FnGraph::rvs_new();
        let mut node = rvs_make_behavior();
        node.rvs_set_expected_public_caps_M(CapabilitySet::rvs_from_validated("P"));
        graph.rvs_insert_M(
            DefPath::from("demo::Adapter::rvs_fetch_BI@demo::Client"),
            node,
        );

        let diffs = rvs_collect_contract_diffs(&graph, &BTreeSet::from([CrateName::from("demo")]));
        let diff = diffs.first().expect("expected trait impl contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_contract_diffs_reads_trait_impl_method_name",
            &format!("diff={diff:?}\n"),
        );

        assert_eq!(diff.actual_name.rvs_as_str(), "rvs_fetch_BI");
        assert_eq!(
            diff.declared_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("BI"))
        );
    }

    #[test]
    fn test_20260703_collect_local_contract_diffs_populates_expected_fields() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::parse"), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();

        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &seed,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let diff = diffs.first().expect("expected one local contract diff");
        rvs_snapshot_BIS(
            "test_20260703_collect_local_contract_diffs_populates_expected_fields",
            &format!("diff={diff:?}\nnode={:?}\n", graph.rvs_get("demo::parse")),
        );

        assert_eq!(
            diff.expected_name.as_ref().map(FnName::rvs_as_str),
            Some("rvs_parse")
        );
        assert_eq!(
            graph
                .rvs_get("demo::parse")
                .and_then(|node| node.expected_name.as_ref())
                .map(FnName::rvs_as_str),
            Some("rvs_parse")
        );
        assert_eq!(
            graph
                .rvs_get("demo::parse")
                .and_then(|node| node.expected_public_caps.as_ref()),
            Some(&CapabilitySet::rvs_new())
        );
    }

    #[test]
    fn test_20260703_enforced_contract_diffs_skip_synthetic_nodes() {
        let mut graph = FnGraph::rvs_new();
        let mut caller = rvs_make_behavior();
        caller.calls.insert(DefPath::from("demo::rvs_generated_BI"));
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), caller);
        let diffs = rvs_collect_local_contract_diffs_M(
            &mut graph,
            &capsmap::CapsMap::rvs_parse("demo::rvs_generated_BI=BI").unwrap(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let enforced = rvs_collect_enforced_contract_diffs(
            &graph,
            &diffs,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let synthetic_diff = diffs
            .iter()
            .find(|diff| diff.def_path.rvs_as_str() == "demo::rvs_generated_BI")
            .expect("synthetic local callee should have a raw diff");
        let synthetic = graph
            .rvs_get("demo::rvs_generated_BI")
            .expect("seeded local callee should be represented for why output");
        rvs_snapshot_BIS(
            "test_20260703_enforced_contract_diffs_skip_synthetic_nodes",
            &format!(
                "synthetic={}\nraw={}\nenforced={enforced:?}\n",
                synthetic.is_synthetic,
                diffs.len(),
            ),
        );

        assert!(synthetic.is_synthetic);
        assert!(!rvs_contract_diff_is_enforced(
            &graph,
            synthetic_diff,
            &BTreeSet::from([CrateName::from("demo")])
        ));
        assert!(
            !enforced
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::rvs_generated_BI")
        );
    }

    #[test]
    fn test_20260703_collect_single_local_contract_diff_port_ignores_async() {
        let node = FnNode {
            calls: BTreeSet::new(),
            facts: CapabilityFacts {
                has_async: true,
                is_port_method: true,
                ..CapabilityFacts::default()
            },
            has_body: true,
            is_trait_impl: false,
            is_test: false,
            sources: BTreeSet::new(),
            report_caps: None,
            report_line_count: None,
            allows_dead_code: false,
            is_synthetic: false,
            expected_public_caps: None,
            expected_name: None,
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
            diff.expected_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("P"))
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
            diff.expected_public_caps.as_ref(),
            Some(&CapabilitySet::rvs_from_validated("P"))
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
                expected_name: Some(FnName::from("rvs_parse")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::rvs_fetch_BI"),
                actual_name: FnName::from("rvs_fetch_BI"),
                expected_name: Some(FnName::from("rvs_fetch_P")),
                declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
                expected_public_caps: Some(CapabilitySet::rvs_from_validated("AP")),
            },
        ];
        let counts = rvs_summarize_contract_mismatches(&diffs);
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
            expected_name: Some(FnName::from("rvs_fetch_P")),
            declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
            expected_public_caps: Some(CapabilitySet::rvs_from_validated("AP")),
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
            expected_name: Some(FnName::from("rvs_parse")),
            declared_public_caps: None,
            expected_public_caps: Some(CapabilitySet::rvs_new()),
        };
        rvs_snapshot_BIS(
            "test_20260703_contract_diff_missing_rvs_prefix",
            &format!("missing={}\n", diff.rvs_missing_rvs_prefix()),
        );

        assert!(diff.rvs_missing_rvs_prefix());
    }

    #[test]
    fn test_20260703_contract_diff_mismatch_kinds() {
        let diff = FnContractDiff {
            def_path: DefPath::from("demo::rvs_fetch_BI"),
            actual_name: FnName::from("rvs_fetch_BI"),
            expected_name: Some(FnName::from("rvs_fetch_P")),
            declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
            expected_public_caps: Some(CapabilitySet::rvs_from_validated("AP")),
        };
        let mismatches = diff.rvs_mismatch_kinds();
        rvs_snapshot_BIS(
            "test_20260703_contract_diff_mismatch_kinds",
            &format!("mismatches={mismatches:?}\n"),
        );

        assert!(mismatches.contains(&FnContractMismatchKind::NameMismatch));
        assert!(mismatches.contains(&FnContractMismatchKind::MissingAsync));
        assert!(mismatches.contains(&FnContractMismatchKind::MissingPort));
        assert!(!mismatches.contains(&FnContractMismatchKind::MissingRvsPrefix));
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
}
