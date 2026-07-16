use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityFacts;
use crate::function_classification::LocalScope;
use crate::symbols::{CrateName, DefPath};

pub(crate) const CALLGRAPH_SCHEMA_VERSION: u32 = 9;

#[derive(Debug, Serialize)]
struct CallgraphArtifactRef<'a> {
    schema_version: u32,
    nodes: &'a FnGraph,
}

#[derive(Debug, Deserialize)]
struct CallgraphArtifact {
    schema_version: u32,
    nodes: FnGraph,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnSource {
    pub file: PathBuf,
    /// Exact rustc working directory for a relative file; absent for absolute or legacy paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base: Option<PathBuf>,
    pub name_start: u32,
    pub name_end: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FunctionIdentity {
    pub(crate) crate_id: u64,
    pub(crate) def_path: DefPath,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CallSiteIdentity {
    pub(crate) callee: FunctionIdentity,
    pub(crate) occurrence: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<CallSiteSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CallSiteSource {
    pub(crate) file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base: Option<PathBuf>,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl FnSource {
    pub(crate) fn rvs_new(file: PathBuf, name_start: u32, name_end: u32) -> Self {
        debug_assert!(name_start < name_end, "source name range must be non-empty");
        Self {
            file,
            base: None,
            name_start,
            name_end,
        }
    }

    pub(crate) fn rvs_new_relative(
        file: PathBuf,
        base: PathBuf,
        name_start: u32,
        name_end: u32,
    ) -> Self {
        debug_assert!(file.is_relative(), "source file must be relative");
        debug_assert!(base.is_absolute(), "source base must be absolute");
        debug_assert!(name_start < name_end, "source name range must be non-empty");
        Self {
            file,
            base: Some(base),
            name_start,
            name_end,
        }
    }
}

impl CallSiteSource {
    pub(crate) fn rvs_new(file: PathBuf, start: u32, end: u32) -> Self {
        debug_assert!(start < end, "call-site source range must be non-empty");
        Self {
            file,
            base: None,
            start,
            end,
        }
    }

    pub(crate) fn rvs_new_relative(file: PathBuf, base: PathBuf, start: u32, end: u32) -> Self {
        debug_assert!(file.is_relative(), "call-site source file must be relative");
        debug_assert!(base.is_absolute(), "call-site source base must be absolute");
        debug_assert!(start < end, "call-site source range must be non-empty");
        Self {
            file,
            base: Some(base),
            start,
            end,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FnNode {
    pub calls: BTreeSet<DefPath>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub entry_calls: BTreeSet<DefPath>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unresolved_test_calls: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) coverage_calls: BTreeMap<u64, BTreeSet<FunctionIdentity>>,
    #[serde(default)]
    pub(crate) coverage_call_sites: BTreeMap<u64, BTreeSet<CallSiteIdentity>>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) test_crate_ids: BTreeSet<u64>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) production_crate_ids: BTreeSet<u64>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) coverage_candidate_crate_ids: BTreeSet<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) sources_by_crate: BTreeMap<u64, BTreeSet<FnSource>>,
    #[serde(default)]
    pub(crate) facts_by_crate: BTreeMap<u64, CapabilityFacts>,
    #[serde(default)]
    pub(crate) has_body_by_crate: BTreeMap<u64, bool>,
    #[serde(default)]
    pub(crate) entrypoint_crate_ids: BTreeSet<u64>,
    #[serde(flatten)]
    pub facts: CapabilityFacts,
    pub has_body: bool,
    #[serde(default)]
    pub is_trait_impl: bool,
    #[serde(default)]
    pub is_test: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub is_entrypoint: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub is_test_compilation: bool,
    #[serde(default)]
    pub sources: BTreeSet<FnSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_line_count: Option<usize>,
    #[serde(default, skip_serializing_if = "rvs_is_zero")]
    pub(crate) report_function_count: usize,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub allows_dead_code: bool,
}

impl Default for FnNode {
    fn default() -> Self {
        Self {
            calls: BTreeSet::new(),
            entry_calls: BTreeSet::new(),
            unresolved_test_calls: BTreeSet::new(),
            coverage_calls: BTreeMap::new(),
            coverage_call_sites: BTreeMap::new(),
            test_crate_ids: BTreeSet::new(),
            production_crate_ids: BTreeSet::new(),
            coverage_candidate_crate_ids: BTreeSet::new(),
            sources_by_crate: BTreeMap::new(),
            facts_by_crate: BTreeMap::new(),
            has_body_by_crate: BTreeMap::new(),
            entrypoint_crate_ids: BTreeSet::new(),
            facts: CapabilityFacts::default(),
            has_body: true,
            is_trait_impl: false,
            is_test: false,
            is_entrypoint: false,
            is_test_compilation: false,
            sources: BTreeSet::new(),
            report_line_count: None,
            report_function_count: 0,
            allows_dead_code: false,
        }
    }
}

impl FnNode {
    fn rvs_merge_coverage_M(&mut self, other: &Self) {
        for (crate_id, calls) in &other.coverage_calls {
            self.calls
                .extend(calls.iter().map(|call| call.def_path.clone()));
            self.coverage_calls
                .entry(*crate_id)
                .or_default()
                .extend(calls.iter().cloned());
        }
        for (crate_id, call_sites) in &other.coverage_call_sites {
            self.coverage_call_sites
                .entry(*crate_id)
                .or_default()
                .extend(call_sites.iter().cloned());
        }
        self.test_crate_ids
            .extend(other.test_crate_ids.iter().copied());
        self.production_crate_ids
            .extend(other.production_crate_ids.iter().copied());
        self.coverage_candidate_crate_ids
            .extend(other.coverage_candidate_crate_ids.iter().copied());
        for (crate_id, sources) in &other.sources_by_crate {
            self.sources_by_crate
                .entry(*crate_id)
                .or_default()
                .extend(sources.iter().cloned());
        }
        for (crate_id, facts) in &other.facts_by_crate {
            self.facts_by_crate
                .entry(*crate_id)
                .and_modify(|existing| {
                    existing.has_async |= facts.has_async;
                    existing.is_unsafe_fn |= facts.is_unsafe_fn;
                    existing.has_mut_param |= facts.has_mut_param;
                    existing.has_static_ref |= facts.has_static_ref;
                    existing.has_static_mut_ref |= facts.has_static_mut_ref;
                    existing.has_thread_local_ref |= facts.has_thread_local_ref;
                    existing.is_port_method |= facts.is_port_method;
                })
                .or_insert(*facts);
        }
        for (crate_id, has_body) in &other.has_body_by_crate {
            self.has_body_by_crate
                .entry(*crate_id)
                .and_modify(|existing| *existing |= *has_body)
                .or_insert(*has_body);
        }
        self.entrypoint_crate_ids
            .extend(other.entrypoint_crate_ids.iter().copied());
    }

    /// Merge another callgraph entry for the same function into this one.
    pub fn rvs_merge_M(&mut self, other: &Self) {
        self.calls.extend(other.calls.iter().cloned());
        self.entry_calls.extend(other.entry_calls.iter().cloned());
        self.unresolved_test_calls
            .extend(other.unresolved_test_calls.iter().cloned());
        self.rvs_merge_coverage_M(other);
        self.facts.has_async |= other.facts.has_async;
        self.facts.is_unsafe_fn |= other.facts.is_unsafe_fn;
        self.facts.has_mut_param |= other.facts.has_mut_param;
        self.facts.has_static_ref |= other.facts.has_static_ref;
        self.facts.has_static_mut_ref |= other.facts.has_static_mut_ref;
        self.facts.has_thread_local_ref |= other.facts.has_thread_local_ref;
        self.facts.is_port_method |= other.facts.is_port_method;
        self.has_body |= other.has_body;
        self.is_trait_impl |= other.is_trait_impl;
        self.is_test |= other.is_test;
        self.is_entrypoint |= other.is_entrypoint;
        self.is_test_compilation |= other.is_test_compilation;
        self.sources.extend(other.sources.iter().cloned());
        self.report_line_count = self.report_line_count.or(other.report_line_count);
        self.report_function_count = self.report_function_count.max(other.report_function_count);
        self.allows_dead_code |= other.allows_dead_code;
    }

    pub(crate) fn rvs_dependency_calls(&self) -> impl Iterator<Item = &DefPath> {
        self.calls.iter().chain(&self.entry_calls)
    }

    pub(crate) fn rvs_is_entrypoint_for_crate(&self, crate_id: u64) -> bool {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        if self.entrypoint_crate_ids.is_empty() {
            self.is_entrypoint
        } else {
            self.entrypoint_crate_ids.contains(&crate_id)
        }
    }
}

fn rvs_is_false(value: &bool) -> bool {
    !*value
}

fn rvs_is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(transparent)]
pub struct FnGraph {
    pub nodes: BTreeMap<DefPath, FnNode>,
}

impl FnGraph {
    pub(crate) fn rvs_new() -> Self {
        Self::default()
    }

    pub(crate) fn rvs_get(&self, path: &str) -> Option<&FnNode> {
        self.nodes.get(path)
    }

    #[cfg(test)]
    pub(crate) fn rvs_get_mut_M(&mut self, path: &DefPath) -> Option<&mut FnNode> {
        self.nodes.get_mut(path)
    }

    pub(crate) fn rvs_iter(&self) -> impl Iterator<Item = (&DefPath, &FnNode)> {
        self.nodes.iter()
    }

    pub(crate) fn rvs_iter_mut_M(&mut self) -> impl Iterator<Item = (&DefPath, &mut FnNode)> {
        self.nodes.iter_mut()
    }

    #[cfg(test)]
    pub(crate) fn rvs_values(&self) -> impl Iterator<Item = &FnNode> {
        self.nodes.values()
    }

    pub(crate) fn rvs_keys(&self) -> impl Iterator<Item = &DefPath> {
        self.nodes.keys()
    }

    pub(crate) fn rvs_test_reachable_identities(&self) -> BTreeSet<FunctionIdentity> {
        let mut covered = BTreeSet::new();
        let mut pending = VecDeque::new();
        for node in self.nodes.values() {
            for crate_id in &node.test_crate_ids {
                if let Some(calls) = node.coverage_calls.get(crate_id) {
                    pending.extend(calls.iter().cloned());
                }
            }
        }
        while let Some(identity) = pending.pop_front() {
            if !covered.insert(identity.clone()) {
                continue;
            }
            if let Some(calls) = self
                .nodes
                .get(&identity.def_path)
                .and_then(|node| node.coverage_calls.get(&identity.crate_id))
            {
                pending.extend(calls.iter().cloned());
            }
        }
        covered
    }

    #[cfg(test)]
    pub(crate) fn rvs_insert_M(&mut self, path: DefPath, node: FnNode) {
        self.nodes.insert(path, node);
    }

    pub(crate) fn rvs_merge_node_M(&mut self, path: DefPath, node: FnNode) {
        if let Some(existing) = self.nodes.get_mut(&path) {
            existing.rvs_merge_M(&node);
        } else {
            self.nodes.insert(path, node);
        }
    }

    #[cfg(test)]
    pub(crate) fn rvs_merge_from_M(&mut self, other: Self) {
        for (path, node) in other.nodes {
            self.rvs_merge_node_M(path, node);
        }
    }

    pub(crate) fn rvs_merge_artifacts(
        artifacts: Vec<Self>,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Result<Self, String> {
        let mut variants: BTreeMap<DefPath, Vec<FnNode>> = BTreeMap::new();
        for artifact in artifacts {
            for (path, node) in artifact.nodes {
                variants.entry(path).or_default().push(node);
            }
        }

        let mut merged = Self::rvs_new();
        let local_scope = LocalScope::rvs_new(local_crate_names);
        for (path, nodes) in variants {
            let mut coverage = FnNode::default();
            for node in &nodes {
                coverage.rvs_merge_coverage_M(node);
            }
            let is_local = local_scope.rvs_contains(&path);
            let has_production_variant = nodes.iter().any(|node| !node.is_test_compilation);
            let non_test_sources: BTreeSet<FnSource> = nodes
                .iter()
                .filter(|node| !node.is_test_compilation)
                .flat_map(|node| node.sources.iter().cloned())
                .collect();
            let retained: Vec<FnNode> = nodes
                .into_iter()
                .filter(|node| {
                    !node.is_test_compilation
                        || (!node.sources.is_empty()
                            && !node
                                .sources
                                .iter()
                                .any(|source| non_test_sources.contains(source)))
                        || (node.sources.is_empty() && !has_production_variant)
                })
                .collect();
            let (mut entries, mut ordinary): (Vec<_>, Vec<_>) =
                retained.into_iter().partition(|node| node.is_entrypoint);
            let entries_are_test_only = entries.iter().all(|node| node.is_test_compilation);
            let ordinary_are_test_only = ordinary.iter().all(|node| node.is_test_compilation);

            let entry_sources: BTreeSet<FnSource> = entries
                .iter()
                .flat_map(|node| node.sources.iter().cloned())
                .collect();
            if ordinary.iter().any(|node| {
                node.sources
                    .iter()
                    .any(|source| entry_sources.contains(source))
            }) {
                return Err(format!(
                    "function {path} is both an executable entry point and an ordinary function at the same source location"
                ));
            }

            let mut selected = if ordinary.is_empty() {
                entries
                    .pop()
                    .expect("never: every merged path has at least one retained node")
            } else {
                ordinary
                    .pop()
                    .expect("never: checked ordinary variants are non-empty")
            };
            if selected.is_entrypoint {
                for entry in entries {
                    selected.rvs_merge_M(&entry);
                }
                selected.is_test_compilation = entries_are_test_only;
            } else {
                for node in ordinary {
                    if is_local
                        && (selected.facts.is_port_method != node.facts.is_port_method
                            || selected.is_trait_impl != node.is_trait_impl
                            || selected.is_test != node.is_test)
                    {
                        return Err(format!(
                            "function {path} has incompatible roles across Cargo targets"
                        ));
                    }
                    let distinct_local_definition = is_local
                        && !selected.sources.is_empty()
                        && !node.sources.is_empty()
                        && selected.sources.is_disjoint(&node.sources);
                    let combined_line_count = if distinct_local_definition {
                        match (selected.report_line_count, node.report_line_count) {
                            (Some(left), Some(right)) => {
                                Some(left.checked_add(right).ok_or_else(|| {
                                    format!("report line count overflow while merging {path}")
                                })?)
                            }
                            (left, right) => left.or(right),
                        }
                    } else {
                        selected.report_line_count.or(node.report_line_count)
                    };
                    let combined_function_count = if distinct_local_definition {
                        let left = selected
                            .report_function_count
                            .max(usize::from(selected.report_line_count.is_some()));
                        let right = node
                            .report_function_count
                            .max(usize::from(node.report_line_count.is_some()));
                        left.checked_add(right).ok_or_else(|| {
                            format!("report function count overflow while merging {path}")
                        })?
                    } else {
                        selected
                            .report_function_count
                            .max(node.report_function_count)
                    };
                    selected.rvs_merge_M(&node);
                    selected.report_line_count = combined_line_count;
                    selected.report_function_count = combined_function_count;
                }
                if selected.report_line_count.is_some() && selected.report_function_count == 0 {
                    selected.report_function_count = 1;
                }
                for entry in entries {
                    selected
                        .entry_calls
                        .extend(entry.rvs_dependency_calls().cloned());
                }
                selected.is_test_compilation = ordinary_are_test_only;
            }
            selected.rvs_merge_coverage_M(&coverage);
            merged.nodes.insert(path, selected);
        }
        Ok(merged)
    }

    #[cfg(test)]
    pub(crate) fn rvs_len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn rvs_is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

pub(crate) fn rvs_serialize_callgraph_json_S(graph: &FnGraph) -> Result<String, String> {
    let artifact = CallgraphArtifactRef {
        schema_version: CALLGRAPH_SCHEMA_VERSION,
        nodes: graph,
    };
    serde_json::to_string(&artifact).map_err(|e| format!("cannot serialize callgraph JSON: {e}"))
}

pub(crate) fn rvs_serialize_function_identities_json_S(
    functions: &BTreeSet<FunctionIdentity>,
) -> Result<String, String> {
    serde_json::to_string(functions)
        .map_err(|error| format!("cannot serialize function identities: {error}"))
}

pub(crate) fn rvs_parse_function_identities_json_S(
    json: &str,
) -> Result<BTreeSet<FunctionIdentity>, String> {
    serde_json::from_str(json).map_err(|error| format!("cannot parse function identities: {error}"))
}

/// Parse versioned or legacy callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json_S(json: &str) -> Result<FnGraph, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "invalid callgraph JSON: root must be an object".to_string())?;
    let is_versioned = object.contains_key("schema_version") || object.contains_key("nodes");
    let graph = if is_versioned {
        let schema_version = object
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                "invalid callgraph artifact: schema_version must be an unsigned integer".to_string()
            })?;
        if schema_version != u64::from(CALLGRAPH_SCHEMA_VERSION) {
            return Err(format!(
                "unsupported callgraph schema version {schema_version}; expected {CALLGRAPH_SCHEMA_VERSION}"
            ));
        }
        let nodes = object
            .get("nodes")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| "invalid callgraph artifact: nodes must be an object".to_string())?;
        for (def_path, node) in nodes {
            let Some(fields) = node.as_object() else {
                return Err(format!(
                    "invalid callgraph artifact: node for {def_path} must be an object"
                ));
            };
            if !fields.contains_key("facts_by_crate") {
                return Err(format!(
                    "invalid callgraph artifact: facts_by_crate is required for {def_path} in schema version {CALLGRAPH_SCHEMA_VERSION}"
                ));
            }
            if !fields.contains_key("has_body_by_crate") {
                return Err(format!(
                    "invalid callgraph artifact: has_body_by_crate is required for {def_path} in schema version {CALLGRAPH_SCHEMA_VERSION}"
                ));
            }
            if !fields.contains_key("coverage_call_sites") {
                return Err(format!(
                    "invalid callgraph artifact: coverage_call_sites is required for {def_path} in schema version {CALLGRAPH_SCHEMA_VERSION}"
                ));
            }
            if !fields.contains_key("entrypoint_crate_ids") {
                return Err(format!(
                    "invalid callgraph artifact: entrypoint_crate_ids is required for {def_path} in schema version {CALLGRAPH_SCHEMA_VERSION}"
                ));
            }
        }
        let artifact: CallgraphArtifact = serde_json::from_value(value)
            .map_err(|e| format!("invalid callgraph artifact: {e}"))?;
        debug_assert_eq!(artifact.schema_version, CALLGRAPH_SCHEMA_VERSION);
        artifact.nodes
    } else {
        for (def_path, node) in object {
            if node
                .as_object()
                .is_some_and(|fields| !fields.contains_key("has_body"))
            {
                return Err(format!(
                    "stale callgraph JSON lacks has_body for {def_path}; delete the stale cache or run cargo rivus infer-std for std cache"
                ));
            }
        }
        serde_json::from_value(value).map_err(|e| format!("invalid callgraph JSON: {e}"))?
    };
    for (path, node) in graph.rvs_iter() {
        if path.rvs_as_str().is_empty() {
            return Err("invalid callgraph JSON: function path is empty".into());
        }
        for callee in node.rvs_dependency_calls() {
            if callee.rvs_as_str().is_empty() {
                return Err(format!(
                    "invalid callgraph JSON: callee path for {path} is empty"
                ));
            }
        }
        if is_versioned {
            let mut crate_ids = node.production_crate_ids.clone();
            crate_ids.extend(node.test_crate_ids.iter().copied());
            crate_ids.extend(node.coverage_candidate_crate_ids.iter().copied());
            crate_ids.extend(node.sources_by_crate.keys().copied());
            crate_ids.extend(node.coverage_calls.keys().copied());
            crate_ids.extend(node.coverage_call_sites.keys().copied());
            crate_ids.extend(node.facts_by_crate.keys().copied());
            crate_ids.extend(node.has_body_by_crate.keys().copied());
            crate_ids.extend(node.entrypoint_crate_ids.iter().copied());
            if crate_ids.contains(&0) {
                return Err(format!(
                    "invalid callgraph artifact: target metadata for {path} contains zero crate id"
                ));
            }
            if crate_ids.is_empty() {
                return Err(format!(
                    "invalid callgraph artifact: node for {path} has no target identity"
                ));
            }
            if let Some(crate_id) = crate_ids
                .iter()
                .find(|crate_id| !node.sources_by_crate.contains_key(crate_id))
            {
                return Err(format!(
                    "invalid callgraph artifact: sources_by_crate for {path} is missing crate id {crate_id}"
                ));
            }
            if let Some(crate_id) = crate_ids
                .iter()
                .find(|crate_id| !node.coverage_calls.contains_key(crate_id))
            {
                return Err(format!(
                    "invalid callgraph artifact: coverage_calls for {path} is missing crate id {crate_id}"
                ));
            }
            if let Some(crate_id) = crate_ids
                .iter()
                .find(|crate_id| !node.coverage_call_sites.contains_key(crate_id))
            {
                return Err(format!(
                    "invalid callgraph artifact: coverage_call_sites for {path} is missing crate id {crate_id}"
                ));
            }
            if let Some(crate_id) = crate_ids
                .iter()
                .find(|crate_id| !node.facts_by_crate.contains_key(crate_id))
            {
                return Err(format!(
                    "invalid callgraph artifact: facts_by_crate for {path} is missing crate id {crate_id}"
                ));
            }
            if let Some(crate_id) = crate_ids
                .iter()
                .find(|crate_id| !node.has_body_by_crate.contains_key(crate_id))
            {
                return Err(format!(
                    "invalid callgraph artifact: has_body_by_crate for {path} is missing crate id {crate_id}"
                ));
            }
            if node.is_entrypoint && node.entrypoint_crate_ids.is_empty() {
                return Err(format!(
                    "invalid callgraph artifact: entrypoint node for {path} has no entrypoint crate identity"
                ));
            }
        }
        let mut all_sources = node.sources.clone();
        all_sources.extend(
            node.sources_by_crate
                .values()
                .flat_map(|sources| sources.iter().cloned()),
        );
        for source in &all_sources {
            if source.file.as_os_str().is_empty() {
                return Err(format!(
                    "invalid callgraph JSON: source file for {path} is empty"
                ));
            }
            if let Some(base) = &source.base {
                if base.as_os_str().is_empty() {
                    return Err(format!(
                        "invalid callgraph JSON: source base for {path} is empty"
                    ));
                }
                if source.file.is_absolute() {
                    return Err(format!(
                        "invalid callgraph JSON: absolute source file for {path} must not have a base"
                    ));
                }
                if !base.is_absolute() {
                    return Err(format!(
                        "invalid callgraph JSON: source base for {path} must be absolute"
                    ));
                }
            }
            if source.name_start >= source.name_end {
                return Err(format!(
                    "invalid callgraph JSON: source range for {path} is empty or reversed ({}..{})",
                    source.name_start, source.name_end
                ));
            }
        }
        if let Some(call) = node
            .coverage_calls
            .values()
            .flatten()
            .find(|call| call.crate_id == 0)
        {
            return Err(format!(
                "invalid callgraph JSON: callee identity for {path} and {} contains zero crate id",
                call.def_path
            ));
        }
        for (crate_id, call_sites) in &node.coverage_call_sites {
            let mut occurrences = BTreeSet::new();
            if let Some(call_site) = call_sites.iter().find(|call_site| {
                call_site.callee.crate_id == 0 || call_site.callee.def_path.rvs_as_str().is_empty()
            }) {
                return Err(format!(
                    "invalid callgraph JSON: call-site callee identity for {path} occurrence {} is invalid",
                    call_site.occurrence
                ));
            }
            if let Some(call_site) = call_sites.iter().find(|call_site| {
                !occurrences.insert((call_site.source.clone(), call_site.occurrence))
            }) {
                return Err(format!(
                    "invalid callgraph artifact: coverage_call_sites for {path} crate id {crate_id} repeats occurrence {}",
                    call_site.occurrence
                ));
            }
            let site_callees: BTreeSet<FunctionIdentity> = call_sites
                .iter()
                .map(|call_site| call_site.callee.clone())
                .collect();
            if node.coverage_calls.get(crate_id) != Some(&site_callees) {
                return Err(format!(
                    "invalid callgraph artifact: coverage_call_sites for {path} crate id {crate_id} does not match coverage_calls"
                ));
            }
            for call_site in call_sites {
                if call_site.source.is_some() {
                    rvs_validate_call_site_source(call_site, path)?;
                }
            }
        }
        if is_versioned {
            let target_call_paths: BTreeSet<DefPath> = node
                .coverage_calls
                .values()
                .flatten()
                .map(|call| call.def_path.clone())
                .collect();
            if node.calls != target_call_paths {
                return Err(format!(
                    "invalid callgraph artifact: aggregate calls for {path} do not match coverage_calls"
                ));
            }
        }
    }
    Ok(graph)
}

fn rvs_validate_call_site_source(
    call_site: &CallSiteIdentity,
    caller: &DefPath,
) -> Result<(), String> {
    let source = call_site
        .source
        .as_ref()
        .expect("never: caller checks that call-site source is present");
    let occurrence = call_site.occurrence;
    if source.file.as_os_str().is_empty() {
        return Err(format!(
            "invalid callgraph JSON: call-site source file for {caller} occurrence {occurrence} is empty"
        ));
    }
    if let Some(base) = &source.base {
        if base.as_os_str().is_empty() {
            return Err(format!(
                "invalid callgraph JSON: call-site source base for {caller} occurrence {occurrence} is empty"
            ));
        }
        if source.file.is_absolute() || !base.is_absolute() {
            return Err(format!(
                "invalid callgraph JSON: call-site source base for {caller} occurrence {occurrence} is inconsistent"
            ));
        }
    }
    if source.start >= source.end {
        return Err(format!(
            "invalid callgraph JSON: call-site source range for {caller} occurrence {occurrence} is empty or reversed"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260609_parse_callgraph_valid_json() {
        let json = r#"{
            "my_crate::rvs_add": {
                "calls": ["my_crate::rvs_helper"],
                "has_body": true,
                "has_async": false,
                                "is_unsafe_fn": false,
                "has_mut_param": false,
                                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            },
            "my_crate::rvs_write_BI": {
                "calls": ["std::fs::write"],
                "report_caps": "BI",
                "has_body": true,
                "has_async": false,
                                "is_unsafe_fn": false,
                "has_mut_param": false,
                                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#;
        let result = rvs_parse_callgraph_json_S(json).unwrap();
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS("test_20260609_parse_callgraph_valid_json", &output);
        assert_eq!(result.rvs_len(), 2);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("report_caps"),
            "legacy report_caps should be accepted but not reserialized"
        );

        let add_behavior = result
            .rvs_get("my_crate::rvs_add")
            .expect("should find rvs_add");
        assert!(add_behavior.calls.contains("my_crate::rvs_helper"));

        let write_behavior = result
            .rvs_get("my_crate::rvs_write_BI")
            .expect("should find rvs_write_BI");
        assert!(write_behavior.calls.contains("std::fs::write"));
        assert_eq!(result.rvs_values().count(), 2);
    }

    #[test]
    fn test_20260710_callgraph_artifact_version_roundtrip() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.production_crate_ids.insert(7);
        node.coverage_candidate_crate_ids.insert(7);
        node.sources_by_crate.insert(7, BTreeSet::new());
        node.coverage_calls.insert(7, BTreeSet::new());
        node.coverage_call_sites.insert(7, BTreeSet::new());
        node.facts_by_crate.insert(7, CapabilityFacts::default());
        node.has_body_by_crate.insert(7, true);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);

        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json_S(&json).unwrap();
        let previous_version = json.replacen(
            &format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#),
            r#""schema_version":3"#,
            1,
        );
        let previous_version_error = rvs_parse_callgraph_json_S(&previous_version).unwrap_err();
        let mut missing_facts: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_facts["nodes"]["demo::rvs_run"]
            .as_object_mut()
            .expect("never: serialized node is an object")
            .remove("facts_by_crate");
        let missing_facts_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_facts).unwrap())
                .unwrap_err();
        let mut missing_has_body: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_has_body["nodes"]["demo::rvs_run"]
            .as_object_mut()
            .expect("never: serialized node is an object")
            .remove("has_body_by_crate");
        let missing_has_body_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_has_body).unwrap())
                .unwrap_err();
        let mut missing_call_sites: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_call_sites["nodes"]["demo::rvs_run"]
            .as_object_mut()
            .expect("never: serialized node is an object")
            .remove("coverage_call_sites");
        let missing_call_sites_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_call_sites).unwrap())
                .unwrap_err();
        let mut missing_entrypoints: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_entrypoints["nodes"]["demo::rvs_run"]
            .as_object_mut()
            .expect("never: serialized node is an object")
            .remove("entrypoint_crate_ids");
        let missing_entrypoints_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_entrypoints).unwrap())
                .unwrap_err();
        let version_marker = format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#);
        let output = format!(
            "schema_version={CALLGRAPH_SCHEMA_VERSION}\ncontains_version={}\nnodes={}\nprevious_version_error={previous_version_error}\nmissing_facts_error={missing_facts_error}\nmissing_has_body_error={missing_has_body_error}\nmissing_call_sites_error={missing_call_sites_error}\nmissing_entrypoints_error={missing_entrypoints_error}\n",
            json.contains(&version_marker),
            parsed.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_version_roundtrip",
            &output,
        );

        assert!(json.contains(r#""nodes""#));
        assert!(parsed.rvs_get("demo::rvs_run").is_some());
    }

    #[test]
    fn test_20260716_callgraph_artifact_rejects_identityless_node() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), FnNode::default());

        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let error = rvs_parse_callgraph_json_S(&json).unwrap_err();
        let output = format!("{error}\n");
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_rejects_identityless_node",
            &output,
        );

        assert!(error.contains("target identity"));
    }

    #[test]
    fn test_20260715_callgraph_artifact_requires_facts_for_each_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.production_crate_ids.insert(7);
        node.sources_by_crate.insert(7, BTreeSet::new());
        node.coverage_calls.insert(7, BTreeSet::new());
        node.coverage_call_sites.insert(7, BTreeSet::new());
        let path = DefPath::from("demo::rvs_run");
        graph.rvs_insert_M(path.clone(), node);

        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let missing_facts_error = rvs_parse_callgraph_json_S(&json).unwrap_err();
        graph
            .rvs_get_mut_M(&path)
            .expect("never: test graph contains the node")
            .facts_by_crate
            .insert(7, CapabilityFacts::default());
        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let missing_has_body_error = rvs_parse_callgraph_json_S(&json).unwrap_err();
        let output = format!(
            "missing_facts={missing_facts_error}\nmissing_has_body={missing_has_body_error}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_callgraph_artifact_requires_facts_for_each_crate_identity",
            &output,
        );

        assert!(missing_facts_error.contains("facts_by_crate"));
        assert!(missing_has_body_error.contains("has_body_by_crate"));
    }

    #[test]
    fn test_20260710_callgraph_artifact_schema_validation() {
        let cases = [
            ("unknown", r#"{"schema_version":10,"nodes":{}}"#),
            ("missing", r#"{"nodes":{}}"#),
            ("string", r#"{"schema_version":"2","nodes":{}}"#),
        ];
        let mut output = String::new();
        for (name, json) in cases {
            let result = rvs_parse_callgraph_json_S(json);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_schema_validation",
            &output,
        );

        assert!(output.contains("unsupported callgraph schema version 10"));
        assert!(output.contains("schema_version must be an unsigned integer"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_validates_target_entrypoints() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode {
            is_entrypoint: true,
            ..FnNode::default()
        };
        node.production_crate_ids.insert(7);
        node.sources_by_crate.insert(7, BTreeSet::new());
        node.coverage_calls.insert(7, BTreeSet::new());
        node.coverage_call_sites.insert(7, BTreeSet::new());
        node.facts_by_crate.insert(7, CapabilityFacts::default());
        node.has_body_by_crate.insert(7, true);
        node.entrypoint_crate_ids.insert(7);
        graph.rvs_insert_M(DefPath::from("demo::main"), node);

        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let valid = rvs_parse_callgraph_json_S(&valid_json);
        let mut missing_identity: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        missing_identity["nodes"]["demo::main"]["entrypoint_crate_ids"] =
            serde_json::Value::Array(Vec::new());
        let missing_identity =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_identity).unwrap())
                .unwrap_err();
        let output = format!(
            "valid={}\nmissing_identity={missing_identity}\n",
            valid.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_validates_target_entrypoints",
            &output,
        );

        assert!(valid.is_ok());
        assert!(missing_identity.contains("no entrypoint crate identity"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_rejects_asymmetric_target_metadata() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.production_crate_ids.insert(7);
        node.sources_by_crate.insert(7, BTreeSet::new());
        node.coverage_calls.insert(7, BTreeSet::new());
        node.coverage_call_sites.insert(7, BTreeSet::new());
        node.facts_by_crate.insert(7, CapabilityFacts::default());
        node.has_body_by_crate.insert(7, true);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);

        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let mut extra_facts: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        extra_facts["nodes"]["demo::rvs_run"]["facts_by_crate"]["8"] =
            serde_json::to_value(CapabilityFacts::default()).unwrap();
        let extra_facts_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&extra_facts).unwrap()).unwrap_err();

        let mut zero_id: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        zero_id["nodes"]["demo::rvs_run"]["facts_by_crate"]["0"] =
            serde_json::to_value(CapabilityFacts::default()).unwrap();
        zero_id["nodes"]["demo::rvs_run"]["has_body_by_crate"]["0"] = true.into();
        zero_id["nodes"]["demo::rvs_run"]["sources_by_crate"]["0"] =
            serde_json::Value::Array(Vec::new());
        zero_id["nodes"]["demo::rvs_run"]["coverage_calls"]["0"] =
            serde_json::Value::Array(Vec::new());
        zero_id["nodes"]["demo::rvs_run"]["coverage_call_sites"]["0"] =
            serde_json::Value::Array(Vec::new());
        let zero_id_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&zero_id).unwrap()).unwrap_err();

        let mut invalid_source: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        invalid_source["nodes"]["demo::rvs_run"]["sources_by_crate"]["7"] =
            serde_json::from_str(r#"[{"file":"","name_start":1,"name_end":2}]"#).unwrap();
        let invalid_source_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&invalid_source).unwrap())
                .unwrap_err();

        let output = format!(
            "extra_facts={extra_facts_error}\nzero_id={zero_id_error}\ninvalid_source={invalid_source_error}\n"
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_rejects_asymmetric_target_metadata",
            &output,
        );

        assert!(extra_facts_error.contains("missing crate id 8"));
        assert!(zero_id_error.contains("zero crate id"));
        assert!(invalid_source_error.contains("source file"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_validates_call_site_coverage_consistency() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.production_crate_ids.insert(7);
        node.sources_by_crate.insert(7, BTreeSet::new());
        node.coverage_calls.insert(
            7,
            BTreeSet::from([FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("dependency::first"),
            }]),
        );
        node.calls.insert(DefPath::from("dependency::first"));
        node.coverage_call_sites.insert(7, BTreeSet::new());
        node.facts_by_crate.insert(7, CapabilityFacts::default());
        node.has_body_by_crate.insert(7, true);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let missing_site =
            rvs_parse_callgraph_json_S(&rvs_serialize_callgraph_json_S(&graph).unwrap())
                .unwrap_err();

        let node = graph
            .rvs_get_mut_M(&DefPath::from("demo::rvs_run"))
            .expect("never: graph contains test node");
        node.coverage_calls
            .get_mut(&7)
            .unwrap()
            .insert(FunctionIdentity {
                crate_id: 10,
                def_path: DefPath::from("dependency::second"),
            });
        node.calls.insert(DefPath::from("dependency::second"));
        node.coverage_call_sites.insert(
            7,
            BTreeSet::from([
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 9,
                        def_path: DefPath::from("dependency::first"),
                    },
                    occurrence: 0,
                    source: None,
                },
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("dependency::second"),
                    },
                    occurrence: 0,
                    source: None,
                },
            ]),
        );
        let duplicate_occurrence =
            rvs_parse_callgraph_json_S(&rvs_serialize_callgraph_json_S(&graph).unwrap())
                .unwrap_err();
        let node = graph
            .rvs_get_mut_M(&DefPath::from("demo::rvs_run"))
            .expect("never: graph contains test node");
        node.coverage_call_sites.insert(
            7,
            BTreeSet::from([
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 9,
                        def_path: DefPath::from("dependency::first"),
                    },
                    occurrence: 0,
                    source: None,
                },
                CallSiteIdentity {
                    callee: FunctionIdentity {
                        crate_id: 10,
                        def_path: DefPath::from("dependency::second"),
                    },
                    occurrence: 1,
                    source: None,
                },
            ]),
        );
        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let mut aggregate_mismatch: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        aggregate_mismatch["nodes"]["demo::rvs_run"]["calls"] =
            serde_json::Value::Array(Vec::new());
        let aggregate_mismatch =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&aggregate_mismatch).unwrap())
                .unwrap_err();
        let output = format!(
            "missing_site={missing_site}\nduplicate_occurrence={duplicate_occurrence}\naggregate_mismatch={aggregate_mismatch}\n"
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_validates_call_site_coverage_consistency",
            &output,
        );

        assert!(missing_site.contains("coverage_call_sites"));
        assert!(duplicate_occurrence.contains("occurrence"));
        assert!(aggregate_mismatch.contains("aggregate calls"));
    }

    #[test]
    fn test_20260714_def_path_selection_roundtrip() {
        let functions = BTreeSet::from([
            FunctionIdentity {
                crate_id: 7,
                def_path: DefPath::from("demo::rvs_alpha"),
            },
            FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("demo::Worker::rvs_run_P"),
            },
        ]);

        let json = rvs_serialize_function_identities_json_S(&functions).unwrap();
        let parsed = rvs_parse_function_identities_json_S(&json).unwrap();
        let output = format!("json={json}\nparsed={parsed:?}\n");
        rvs_snapshot_BIS("test_20260714_def_path_selection_roundtrip", &output);

        assert_eq!(parsed, functions);
        assert!(rvs_is_false(&false));
        assert!(!rvs_is_false(&true));
    }

    #[test]
    fn test_20260703_graph_extend_and_merge_helpers() {
        assert!(rvs_is_zero(&0));
        assert!(!rvs_is_zero(&1));
        let mut base = FnGraph::rvs_new();
        base.rvs_insert_M(DefPath::from("demo::rvs_a"), FnNode::default());
        let mut extended = FnGraph::rvs_new();
        extended.rvs_insert_M(DefPath::from("demo::rvs_b"), FnNode::default());
        base.rvs_merge_from_M(extended);

        let mut merged = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.calls.insert(DefPath::from("std::fs::read_to_string"));
        merged.rvs_insert_M(DefPath::from("demo::rvs_a"), node);
        base.rvs_merge_from_M(merged);

        let output = format!(
            "len={}\nmerged_calls={}\n",
            base.rvs_len(),
            base.rvs_get("demo::rvs_a")
                .map(|node| node.calls.len())
                .unwrap_or(0)
        );
        rvs_snapshot_BIS("test_20260703_graph_extend_and_merge_helpers", &output);

        assert_eq!(base.rvs_len(), 2);
        assert_eq!(
            base.rvs_get("demo::rvs_a")
                .map(|node| node.calls.len())
                .unwrap_or(0),
            1
        );
    }

    #[test]
    fn test_20260713_artifact_merge_resolves_entrypoint_roles() {
        let local_crate_names = BTreeSet::from([CrateName::from("demo")]);
        let ordinary_source = FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 11);
        let entry_source = FnSource::rvs_new(PathBuf::from("src/main.rs"), 3, 7);
        let mut ordinary = FnNode {
            sources: BTreeSet::from([ordinary_source.clone()]),
            ..FnNode::default()
        };
        ordinary
            .calls
            .insert(DefPath::from("std::fs::read_to_string"));
        ordinary.production_crate_ids.insert(1);
        let mut entry = FnNode {
            is_entrypoint: true,
            sources: BTreeSet::from([entry_source]),
            ..FnNode::default()
        };
        entry.calls.insert(DefPath::from("std::process::exit"));
        entry.production_crate_ids.insert(2);
        entry.entrypoint_crate_ids.insert(2);

        let mut ordinary_graph = FnGraph::rvs_new();
        ordinary_graph.rvs_insert_M(DefPath::from("demo::main"), ordinary.clone());
        let mut entry_graph = FnGraph::rvs_new();
        entry_graph.rvs_insert_M(DefPath::from("demo::main"), entry.clone());
        let mut test_copy_graph = FnGraph::rvs_new();
        let mut test_copy = FnNode {
            is_test_compilation: true,
            sources: entry.sources.clone(),
            ..FnNode::default()
        };
        test_copy
            .calls
            .insert(DefPath::from("test::test_main_static"));
        test_copy.test_crate_ids.insert(3);
        test_copy_graph.rvs_insert_M(DefPath::from("demo::main"), test_copy);
        let merged = FnGraph::rvs_merge_artifacts(
            vec![ordinary_graph, entry_graph.clone(), test_copy_graph.clone()],
            &local_crate_names,
        )
        .unwrap();
        let retained = merged.rvs_get("demo::main").unwrap();

        let mut incoming_ordinary = FnGraph::rvs_new();
        incoming_ordinary.rvs_insert_M(DefPath::from("demo::main"), ordinary.clone());
        let reverse = FnGraph::rvs_merge_artifacts(
            vec![test_copy_graph, entry_graph, incoming_ordinary],
            &local_crate_names,
        )
        .unwrap();
        let reverse_retained = reverse.rvs_get("demo::main").unwrap();

        let mut shared_entry = FnNode {
            is_entrypoint: true,
            sources: BTreeSet::from([ordinary_source]),
            ..FnNode::default()
        };
        shared_entry
            .calls
            .insert(DefPath::from("std::process::exit"));
        let mut conflict = FnGraph::rvs_new();
        conflict.rvs_insert_M(DefPath::from("demo::main"), ordinary);
        let mut conflicting_artifact = FnGraph::rvs_new();
        conflicting_artifact.rvs_insert_M(DefPath::from("demo::main"), shared_entry);
        let conflict_result =
            FnGraph::rvs_merge_artifacts(vec![conflict, conflicting_artifact], &local_crate_names);

        let mut first_ordinary = FnGraph::rvs_new();
        first_ordinary.rvs_insert_M(
            DefPath::from("demo::rvs_run"),
            FnNode {
                sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 20, 27)]),
                report_line_count: Some(2),
                report_function_count: 1,
                ..FnNode::default()
            },
        );
        let mut conflicting_ordinary_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/main.rs"), 20, 27)]),
            report_line_count: Some(3),
            report_function_count: 1,
            ..FnNode::default()
        };
        conflicting_ordinary_node
            .calls
            .insert(DefPath::from("dep::effect_S"));
        let mut second_ordinary = FnGraph::rvs_new();
        second_ordinary.rvs_insert_M(DefPath::from("demo::rvs_run"), conflicting_ordinary_node);
        let ordinary_merge =
            FnGraph::rvs_merge_artifacts(vec![first_ordinary, second_ordinary], &local_crate_names);
        let ordinary_merged = ordinary_merge
            .as_ref()
            .unwrap()
            .rvs_get("demo::rvs_run")
            .unwrap();

        let mut ordinary_variant = FnGraph::rvs_new();
        ordinary_variant.rvs_insert_M(
            DefPath::from("demo::rvs_fetch"),
            FnNode {
                sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 30, 39)]),
                ..FnNode::default()
            },
        );
        let mut port_variant_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/main.rs"), 30, 39)]),
            ..FnNode::default()
        };
        port_variant_node.facts.is_port_method = true;
        let mut port_variant = FnGraph::rvs_new();
        port_variant.rvs_insert_M(DefPath::from("demo::rvs_fetch"), port_variant_node);
        let mixed_role =
            FnGraph::rvs_merge_artifacts(vec![ordinary_variant, port_variant], &local_crate_names);

        let mut source_less_ordinary = FnGraph::rvs_new();
        source_less_ordinary
            .rvs_insert_M(DefPath::from("demo::rvs_source_less"), FnNode::default());
        let mut source_less_port_node = FnNode::default();
        source_less_port_node.facts.is_port_method = true;
        let mut source_less_port = FnGraph::rvs_new();
        source_less_port.rvs_insert_M(
            DefPath::from("demo::rvs_source_less"),
            source_less_port_node,
        );
        let source_less_mixed_role = FnGraph::rvs_merge_artifacts(
            vec![source_less_ordinary, source_less_port],
            &local_crate_names,
        );

        let output = format!(
            "retained_entry={}\nentrypoint_crate_ids={:?}\nretained_sources={:?}\nretained_calls={:?}\nentry_calls={:?}\nreverse_equal={}\nentry_conflict={conflict_result:?}\nordinary_calls={:?}\nordinary_function_count={}\nordinary_line_count={:?}\nmixed_role={mixed_role:?}\nsource_less_mixed_role={source_less_mixed_role:?}\n",
            retained.is_entrypoint,
            retained.entrypoint_crate_ids,
            retained.sources,
            retained.calls,
            retained.entry_calls,
            retained.sources == reverse_retained.sources
                && retained.calls == reverse_retained.calls
                && retained.entry_calls == reverse_retained.entry_calls
                && retained.is_entrypoint == reverse_retained.is_entrypoint
                && retained.entrypoint_crate_ids == reverse_retained.entrypoint_crate_ids,
            ordinary_merged.calls,
            ordinary_merged.report_function_count,
            ordinary_merged.report_line_count,
        );
        rvs_snapshot_BIS(
            "test_20260713_artifact_merge_resolves_entrypoint_roles",
            &output,
        );

        assert!(!retained.is_entrypoint);
        assert_eq!(retained.entrypoint_crate_ids, BTreeSet::from([2]));
        assert_eq!(retained.sources.len(), 1);
        assert!(retained.calls.contains("std::fs::read_to_string"));
        assert!(!retained.calls.contains("std::process::exit"));
        assert!(retained.entry_calls.contains("std::process::exit"));
        assert!(!retained.entry_calls.contains("test::test_main_static"));
        assert_eq!(retained.sources, reverse_retained.sources);
        assert_eq!(retained.calls, reverse_retained.calls);
        assert_eq!(retained.entry_calls, reverse_retained.entry_calls);
        assert!(conflict_result.is_err());
        assert!(ordinary_merge.is_ok());
        assert!(ordinary_merged.calls.contains("dep::effect_S"));
        assert_eq!(ordinary_merged.report_function_count, 2);
        assert_eq!(ordinary_merged.report_line_count, Some(5));
        assert!(mixed_role.is_err());
        assert!(source_less_mixed_role.is_err());
    }

    #[test]
    fn test_20260715_artifact_merge_treats_local_trait_external_type_impl_as_local() {
        let path = DefPath::from(
            "std::fs::File{impl#7374643a3a66733a3a46696c654064656d6f3a3a46696c65436c69656e74}::rvs_touch_P@demo::FileClient",
        );
        let mut first = FnGraph::rvs_new();
        first.rvs_insert_M(
            path.clone(),
            FnNode {
                is_trait_impl: true,
                sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
                report_line_count: Some(2),
                report_function_count: 1,
                ..FnNode::default()
            },
        );
        let mut second = FnGraph::rvs_new();
        second.rvs_insert_M(
            path.clone(),
            FnNode {
                is_trait_impl: true,
                sources: BTreeSet::from([FnSource::rvs_new("src/other.rs".into(), 1, 2)]),
                report_line_count: Some(3),
                report_function_count: 1,
                ..FnNode::default()
            },
        );

        let merged = FnGraph::rvs_merge_artifacts(
            vec![first, second],
            &BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let node = merged.rvs_get(path.rvs_as_str()).unwrap();
        let output = format!(
            "function_count={}\nline_count={:?}\n",
            node.report_function_count, node.report_line_count
        );
        rvs_snapshot_BIS(
            "test_20260715_artifact_merge_treats_local_trait_external_type_impl_as_local",
            &output,
        );

        assert_eq!(node.report_function_count, 2);
        assert_eq!(node.report_line_count, Some(5));
    }

    #[test]
    fn test_20260714_source_less_production_node_survives_test_merge() {
        let local_crate_names = BTreeSet::from([CrateName::from("demo")]);
        let mut production = FnNode::default();
        production.production_crate_ids.insert(1);
        production.coverage_candidate_crate_ids.insert(1);
        production.sources_by_crate.insert(1, BTreeSet::new());
        let mut test_copy = FnNode {
            is_test_compilation: true,
            ..FnNode::default()
        };
        test_copy
            .calls
            .insert(DefPath::from("demo::rvs_test_only_dependency"));
        test_copy.sources_by_crate.insert(2, BTreeSet::new());
        let mut production_graph = FnGraph::rvs_new();
        production_graph.rvs_insert_M(DefPath::from("demo::rvs_generated"), production);
        let mut test_graph = FnGraph::rvs_new();
        test_graph.rvs_insert_M(DefPath::from("demo::rvs_generated"), test_copy);

        let merged =
            FnGraph::rvs_merge_artifacts(vec![production_graph, test_graph], &local_crate_names)
                .unwrap();
        let node = merged.rvs_get("demo::rvs_generated").unwrap();
        let output = format!(
            "is_test_compilation={}\nsources={}\nproduction_ids={:?}\n",
            node.is_test_compilation,
            node.sources.len(),
            node.production_crate_ids,
        );
        rvs_snapshot_BIS(
            "test_20260714_source_less_production_node_survives_test_merge",
            &output,
        );

        assert!(!node.is_test_compilation);
        assert!(node.sources.is_empty());
        assert_eq!(node.production_crate_ids, BTreeSet::from([1]));
    }

    #[test]
    fn test_20260706_graph_merge_node_preserves_body_and_sources() {
        let mut graph = FnGraph::rvs_new();
        let path = DefPath::from("demo::rvs_run");
        let mut bodyless = FnNode {
            has_body: false,
            ..FnNode::default()
        };
        bodyless.facts.has_async = true;
        graph.rvs_merge_node_M(path.clone(), bodyless);

        let mut with_body = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 14)]),
            ..FnNode::default()
        };
        with_body.calls.insert(DefPath::from("dep::rvs_call_BI"));
        graph.rvs_merge_node_M(path.clone(), with_body);

        let node = graph
            .rvs_get(path.rvs_as_str())
            .expect("merged node exists");
        let output = format!(
            "has_body={}\nhas_async={}\nsources={}\ncalls={}\n",
            node.has_body,
            node.facts.has_async,
            node.sources.len(),
            node.calls.len()
        );
        rvs_snapshot_BIS(
            "test_20260706_graph_merge_node_preserves_body_and_sources",
            &output,
        );

        assert!(node.has_body);
        assert!(node.facts.has_async);
        assert_eq!(node.sources.len(), 1);
        assert_eq!(node.calls.len(), 1);
    }

    #[test]
    fn test_20260710_fn_source_provenance_json_compatibility() {
        let legacy_json = r#"{
            "demo::rvs_parse": {
                "calls": [],
                "has_body": true,
                "sources": [{"file":"src/lib.rs","name_start":7,"name_end":16}]
            }
        }"#;
        let legacy = rvs_parse_callgraph_json_S(legacy_json).unwrap();
        let legacy_source = legacy
            .rvs_get("demo::rvs_parse")
            .and_then(|node| node.sources.first())
            .expect("legacy source should parse");
        let exact_source = FnSource::rvs_new_relative(
            PathBuf::from("member/src/lib.rs"),
            PathBuf::from("/workspace"),
            7,
            16,
        );
        let legacy_serialized = serde_json::to_string(legacy_source).unwrap();
        let exact_serialized = serde_json::to_string(&exact_source).unwrap();
        let output = format!(
            "legacy_base={:?}\nlegacy_json={legacy_serialized}\nexact_base={:?}\nexact_json={exact_serialized}\n",
            legacy_source.base, exact_source.base,
        );
        rvs_snapshot_BIS(
            "test_20260710_fn_source_provenance_json_compatibility",
            &output,
        );

        assert!(legacy_source.base.is_none());
        assert!(!legacy_serialized.contains("base"));
        assert_eq!(exact_source.base, Some(PathBuf::from("/workspace")));
        assert!(exact_serialized.contains(r#""base":"/workspace""#));
    }

    #[test]
    fn test_20260710_parse_callgraph_rejects_invalid_source_provenance() {
        let cases = [
            (
                "relative_base",
                r#"{"file":"src/lib.rs","base":"workspace","name_start":3,"name_end":8}"#,
            ),
            (
                "base_on_absolute_file",
                r#"{"file":"/workspace/src/lib.rs","base":"/workspace","name_start":3,"name_end":8}"#,
            ),
            (
                "empty_base",
                r#"{"file":"src/lib.rs","base":"","name_start":3,"name_end":8}"#,
            ),
        ];
        let mut output = String::new();
        for (name, source) in cases {
            let json = format!(
                r#"{{"demo::rvs_parse":{{"calls":[],"has_body":true,"sources":[{source}]}}}}"#
            );
            let result = rvs_parse_callgraph_json_S(&json);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260710_parse_callgraph_rejects_invalid_source_provenance",
            &output,
        );
    }

    #[test]
    fn test_20260710_fn_source_ordering_keeps_relative_provenance_distinct() {
        let member_source = FnSource::rvs_new_relative(
            PathBuf::from("src/lib.rs"),
            PathBuf::from("/workspace/member"),
            7,
            16,
        );
        let workspace_source = FnSource::rvs_new_relative(
            PathBuf::from("src/lib.rs"),
            PathBuf::from("/workspace"),
            7,
            16,
        );
        let sources = BTreeSet::from([member_source.clone(), workspace_source.clone()]);
        let output = sources
            .iter()
            .map(|source| {
                format!(
                    "file={} base={} range={}..{}",
                    source.file.display(),
                    source
                        .base
                        .as_deref()
                        .map_or("<none>".into(), |base| base.display().to_string()),
                    source.name_start,
                    source.name_end,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260710_fn_source_ordering_keeps_relative_provenance_distinct",
            &(output + "\n"),
        );

        assert_ne!(member_source, workspace_source);
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_20260709_parse_callgraph_rejection_table() {
        let cases = [
            ("invalid_json", "this is not json at all"),
            (
                "missing_has_body",
                r#"{
            "my_crate::rvs_add": {
                "calls": [],
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#,
            ),
            (
                "reversed_source_range",
                r#"{
            "my_crate::rvs_add": {
                "calls": [],
                "has_body": true,
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false,
                "sources": [{"file":"src/lib.rs","name_start":10,"name_end":3}]
            }
        }"#,
            ),
            (
                "empty_source_file",
                r#"{
            "my_crate::rvs_add": {
                "calls": [],
                "has_body": true,
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false,
                "sources": [{"file":"","name_start":3,"name_end":10}]
            }
        }"#,
            ),
            (
                "empty_source_range",
                r#"{
            "my_crate::rvs_add": {
                "calls": [],
                "has_body": true,
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false,
                "sources": [{"file":"src/lib.rs","name_start":10,"name_end":10}]
            }
        }"#,
            ),
            (
                "empty_function_path",
                r#"{
            "": {
                "calls": [],
                "has_body": true,
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#,
            ),
            (
                "empty_callee_path",
                r#"{
            "my_crate::rvs_add": {
                "calls": [""],
                "has_body": true,
                "has_async": false,
                "is_unsafe_fn": false,
                "has_mut_param": false,
                "has_static_ref": false,
                "has_static_mut_ref": false,
                "has_thread_local_ref": false,
                "is_trait_impl": false
            }
        }"#,
            ),
        ];
        let mut output = String::new();
        for (name, json) in cases {
            let result = rvs_parse_callgraph_json_S(json);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS("test_20260709_parse_callgraph_rejection_table", &output);
    }
}
