use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::capability::{CapabilityFacts, rvs_merge_capability_facts_M};
#[allow(
    unused_imports,
    reason = "LocalScope re-exported for downstream callers"
)]
use crate::function_classification::LocalScope;
use crate::symbols::{CrateName, DefPath};

pub(crate) const CALLGRAPH_SCHEMA_VERSION: u32 = 18;

#[derive(Debug, Snafu)]
pub enum CallgraphArtifactError {
    #[snafu(display("invalid callgraph JSON: {source}"))]
    InvalidJson { source: serde_json::Error },
    #[snafu(display("invalid callgraph JSON: root must be an object"))]
    RootMustBeObject,
    #[snafu(display("invalid callgraph artifact: schema_version must be an unsigned integer"))]
    InvalidSchemaVersion,
    #[snafu(display("unsupported callgraph schema version {actual}; expected {expected}"))]
    UnsupportedSchemaVersion { actual: u64, expected: u32 },
    #[snafu(display("invalid callgraph artifact: {source}"))]
    InvalidVersionedRecord { source: serde_json::Error },
    #[snafu(display("invalid legacy callgraph JSON: {source}"))]
    InvalidLegacyRecord { source: serde_json::Error },
    #[snafu(display(
        "stale callgraph JSON lacks has_body for {def_path}; delete the stale cache or run cargo rivus infer-std for std cache"
    ))]
    StaleLegacyMissingHasBody { def_path: DefPath },
    #[snafu(display("cannot serialize a headerless legacy callgraph as current schema truth"))]
    LegacySerialization,
    #[snafu(display(
        "callgraph artifact for {def_path} crate id {crate_id} has unknown package provenance"
    ))]
    UnknownCrateProvenance { def_path: DefPath, crate_id: u64 },
    #[snafu(display(
        "stable crate id {crate_id} has conflicting package provenance: {existing:?} at {existing_path} and {incoming:?} at {incoming_path}"
    ))]
    ConflictingCrateProvenance {
        crate_id: u64,
        existing: CrateProvenance,
        existing_path: DefPath,
        incoming: CrateProvenance,
        incoming_path: DefPath,
    },
    #[snafu(display(
        "cannot merge {legacy_count} legacy callgraph artifact(s) with {current_count} current artifact(s)"
    ))]
    MixedArtifactFormats {
        legacy_count: usize,
        current_count: usize,
    },
    #[snafu(display(
        "invalid callgraph JSON: callee identity for {caller} and {callee} contains zero crate id"
    ))]
    ZeroCalleeCrateId { caller: DefPath, callee: DefPath },
    #[snafu(display("invalid callgraph JSON: source file for {def_path} is empty"))]
    EmptySourceFile { def_path: DefPath },
    #[snafu(display("invalid callgraph JSON: source base for {def_path} is empty"))]
    EmptySourceBase { def_path: DefPath },
    #[snafu(display(
        "invalid callgraph JSON: absolute source file for {def_path} must not have a base"
    ))]
    AbsoluteSourceWithBase { def_path: DefPath },
    #[snafu(display("invalid callgraph JSON: source base for {def_path} must be absolute"))]
    RelativeSourceBase { def_path: DefPath },
    #[snafu(display(
        "invalid callgraph JSON: source range for {def_path} is empty or reversed ({start}..{end})"
    ))]
    InvalidSourceRange {
        def_path: DefPath,
        start: u32,
        end: u32,
    },
    #[snafu(display(
        "invalid callgraph JSON: call-site callee identity for {caller} occurrence {occurrence} is invalid"
    ))]
    InvalidCallSiteCallee { caller: DefPath, occurrence: u32 },
    #[snafu(display(
        "invalid callgraph artifact: call_sites for {caller} repeats occurrence {occurrence}"
    ))]
    RepeatedCallOccurrence { caller: DefPath, occurrence: u32 },
    #[snafu(display("invalid callgraph artifact: call sites for {caller} do not match calls"))]
    TargetCallsMismatch { caller: DefPath },
    #[snafu(display(
        "invalid callgraph JSON: call-site source file for {caller} occurrence {occurrence} is empty"
    ))]
    EmptyCallSiteSourceFile { caller: DefPath, occurrence: u32 },
    #[snafu(display(
        "invalid callgraph JSON: call-site source base for {caller} occurrence {occurrence} is empty"
    ))]
    EmptyCallSiteSourceBase { caller: DefPath, occurrence: u32 },
    #[snafu(display(
        "invalid callgraph JSON: call-site source base for {caller} occurrence {occurrence} is inconsistent"
    ))]
    InconsistentCallSiteSourceBase { caller: DefPath, occurrence: u32 },
    #[snafu(display(
        "invalid callgraph JSON: call-site source range for {caller} occurrence {occurrence} is empty or reversed"
    ))]
    InvalidCallSiteSourceRange { caller: DefPath, occurrence: u32 },
    #[snafu(display("invalid callgraph JSON: function path is empty"))]
    EmptyFunctionPath,
    #[snafu(display("invalid callgraph JSON: callee path for {caller} is empty"))]
    EmptyCalleePath { caller: DefPath },
    #[snafu(display("invalid callgraph JSON: node for {def_path} contains zero crate id"))]
    ZeroCrateId { def_path: DefPath },
    #[snafu(display("cannot serialize callgraph JSON: {source}"))]
    SerializeCallgraph { source: serde_json::Error },
    #[snafu(display("cannot serialize function identities: {source}"))]
    SerializeFunctionIdentities { source: serde_json::Error },
    #[snafu(display("cannot parse function identities: {source}"))]
    ParseFunctionIdentities { source: serde_json::Error },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CallgraphFormat {
    #[default]
    Current,
    Legacy,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallgraphArtifact {
    schema_version: u32,
    nodes: BTreeMap<DefPath, FnNodeArtifact>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FnNodeArtifact {
    #[serde(
        serialize_with = "rvs_serialize_call_edges",
        deserialize_with = "rvs_deserialize_call_edges"
    )]
    calls: BTreeMap<FunctionIdentity, CallEdgeType>,
    call_sites: BTreeSet<CallSiteIdentity>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    unresolved_test_calls: BTreeSet<String>,
    #[serde(flatten)]
    facts: CapabilityFacts,
    has_body: bool,
    #[serde(default)]
    is_trait_impl: bool,
    #[serde(default)]
    is_test: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    is_entrypoint: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    is_test_compilation: bool,
    #[serde(default)]
    sources: BTreeSet<FnSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    report_line_count: Option<usize>,
    #[serde(default, skip_serializing_if = "rvs_is_zero")]
    report_function_count: usize,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    allows_dead_code: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    is_production: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    is_coverage_candidate: bool,
    crate_provenance: CrateProvenance,
    /// Defining crate identity for this node.
    crate_id: u64,
}

#[derive(Debug, Deserialize, Default)]
struct LegacyCapabilityFacts {
    #[serde(default)]
    has_async: bool,
    #[serde(default)]
    is_unsafe_fn: bool,
    #[serde(default)]
    has_mut_param: bool,
    #[serde(default)]
    has_static_ref: bool,
    #[serde(default)]
    has_static_mut_ref: bool,
    #[serde(default)]
    has_thread_local_ref: bool,
    #[serde(default)]
    is_port_method: bool,
}

impl From<LegacyCapabilityFacts> for CapabilityFacts {
    fn from(facts: LegacyCapabilityFacts) -> Self {
        Self {
            has_async: facts.has_async,
            has_const: false,
            is_unsafe_fn: facts.is_unsafe_fn,
            has_mut_param: facts.has_mut_param,
            has_static_ref: facts.has_static_ref,
            has_static_mut_ref: facts.has_static_mut_ref,
            has_thread_local_ref: facts.has_thread_local_ref,
            is_port_method: facts.is_port_method,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct LegacyFnNodeArtifact {
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "legacy field retained for deserialization compatibility"
    )]
    calls: BTreeSet<DefPath>,
    #[serde(default)]
    entry_calls: BTreeSet<DefPath>,
    #[serde(default)]
    unresolved_test_calls: BTreeSet<String>,
    #[serde(default)]
    coverage_calls: BTreeMap<u64, BTreeSet<FunctionIdentity>>,
    #[serde(default)]
    coverage_call_sites: BTreeMap<u64, BTreeSet<CallSiteIdentity>>,
    #[serde(default)]
    test_crate_ids: BTreeSet<u64>,
    #[serde(default)]
    production_crate_ids: BTreeSet<u64>,
    #[serde(default)]
    coverage_candidate_crate_ids: BTreeSet<u64>,
    #[serde(default)]
    sources_by_crate: BTreeMap<u64, BTreeSet<FnSource>>,
    #[serde(default)]
    facts_by_crate: BTreeMap<u64, LegacyCapabilityFacts>,
    #[serde(default)]
    has_body_by_crate: BTreeMap<u64, bool>,
    #[serde(default)]
    entrypoint_crate_ids: BTreeSet<u64>,
    #[serde(flatten)]
    facts: LegacyCapabilityFacts,
    has_body: bool,
    #[serde(default)]
    is_trait_impl: bool,
    #[serde(default)]
    is_test: bool,
    #[serde(default)]
    is_entrypoint: bool,
    #[serde(default)]
    is_test_compilation: bool,
    #[serde(default)]
    sources: BTreeSet<FnSource>,
    #[serde(default)]
    report_line_count: Option<usize>,
    #[serde(default)]
    report_function_count: usize,
    #[serde(default)]
    allows_dead_code: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub struct FnSource {
    pub file: PathBuf,
    /// Exact rustc working directory for a relative file; absent for absolute or legacy paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base: Option<PathBuf>,
    pub name_start: u32,
    pub name_end: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct FunctionIdentity {
    pub(crate) crate_id: u64,
    pub(crate) def_path: DefPath,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallEdgeType {
    Strong,
    Weak,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CrateProvenance {
    PrimaryPackage,
    Dependency,
    #[default]
    LegacyUnknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallSiteIdentity {
    pub(crate) callee: FunctionIdentity,
    pub(crate) occurrence: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<CallSiteSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallSiteSource {
    pub(crate) file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base: Option<PathBuf>,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

fn rvs_serialize_call_edges<S>(
    calls: &BTreeMap<FunctionIdentity, CallEdgeType>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(calls.len()))?;
    for (identity, edge_type) in calls {
        seq.serialize_element(&CallEdgeRecord {
            callee: identity.clone(),
            edge_type: *edge_type,
        })?;
    }
    seq.end()
}

fn rvs_deserialize_call_edges<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<FunctionIdentity, CallEdgeType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let records = Vec::<CallEdgeRecord>::deserialize(deserializer)?;
    Ok(records
        .into_iter()
        .map(|record| (record.callee, record.edge_type))
        .collect())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct CallEdgeRecord {
    callee: FunctionIdentity,
    edge_type: CallEdgeType,
}

impl FnNodeArtifact {
    fn rvs_into_node(self) -> FnNode {
        FnNode {
            calls: self.calls,
            entry_calls: BTreeMap::new(),
            call_sites: self.call_sites,
            unresolved_test_calls: self.unresolved_test_calls,
            facts: self.facts,
            has_body: self.has_body,
            is_trait_impl: self.is_trait_impl,
            is_test: self.is_test,
            is_entrypoint: self.is_entrypoint,
            is_test_compilation: self.is_test_compilation,
            sources: self.sources,
            report_line_count: self.report_line_count,
            report_function_count: self.report_function_count,
            allows_dead_code: self.allows_dead_code,
            is_production: self.is_production,
            is_coverage_candidate: self.is_coverage_candidate,
            crate_provenance: self.crate_provenance,
            crate_id: self.crate_id,
            complete: true,
        }
    }

    fn rvs_from_node(node: &FnNode) -> Self {
        Self {
            calls: node.calls.clone(),
            call_sites: node.call_sites.clone(),
            unresolved_test_calls: node.unresolved_test_calls.clone(),
            facts: node.facts,
            has_body: node.has_body,
            is_trait_impl: node.is_trait_impl,
            is_test: node.is_test,
            is_entrypoint: node.is_entrypoint,
            is_test_compilation: node.is_test_compilation,
            sources: node.sources.clone(),
            report_line_count: node.report_line_count,
            report_function_count: node.report_function_count,
            allows_dead_code: node.allows_dead_code,
            is_production: node.is_production,
            is_coverage_candidate: node.is_coverage_candidate,
            crate_provenance: node.crate_provenance,
            crate_id: node.crate_id,
        }
    }
}

impl LegacyFnNodeArtifact {
    fn rvs_into_node(self) -> FnNode {
        let _legacy_identity_metadata = (
            self.coverage_calls,
            self.coverage_call_sites,
            self.test_crate_ids,
            self.production_crate_ids,
            self.coverage_candidate_crate_ids,
            self.sources_by_crate,
            self.facts_by_crate,
            self.has_body_by_crate,
            self.entrypoint_crate_ids,
        );
        FnNode {
            calls: self
                .calls
                .into_iter()
                .map(|path| {
                    (
                        FunctionIdentity {
                            crate_id: 0,
                            def_path: path,
                        },
                        CallEdgeType::Strong,
                    )
                })
                .collect(),
            entry_calls: self
                .entry_calls
                .into_iter()
                .map(|path| (path, CallEdgeType::Strong))
                .collect(),
            call_sites: BTreeSet::new(),
            unresolved_test_calls: self.unresolved_test_calls,
            facts: self.facts.into(),
            has_body: self.has_body,
            is_trait_impl: self.is_trait_impl,
            is_test: self.is_test,
            is_entrypoint: self.is_entrypoint,
            is_test_compilation: self.is_test_compilation,
            sources: self.sources,
            report_line_count: self.report_line_count,
            report_function_count: self.report_function_count,
            allows_dead_code: self.allows_dead_code,
            is_production: false,
            is_coverage_candidate: false,
            crate_provenance: CrateProvenance::LegacyUnknown,
            crate_id: 0,
            complete: false,
        }
    }
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
    #[serde(
        serialize_with = "rvs_serialize_call_edges",
        deserialize_with = "rvs_deserialize_call_edges"
    )]
    pub calls: BTreeMap<FunctionIdentity, CallEdgeType>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entry_calls: BTreeMap<DefPath, CallEdgeType>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub call_sites: BTreeSet<CallSiteIdentity>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unresolved_test_calls: BTreeSet<String>,
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
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub is_production: bool,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub is_coverage_candidate: bool,
    #[serde(default)]
    pub crate_provenance: CrateProvenance,
    #[serde(default)]
    pub crate_id: u64,
    #[serde(skip)]
    pub(crate) complete: bool,
}

impl Default for FnNode {
    fn default() -> Self {
        Self {
            calls: BTreeMap::new(),
            entry_calls: BTreeMap::new(),
            call_sites: BTreeSet::new(),
            unresolved_test_calls: BTreeSet::new(),
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
            is_production: false,
            is_coverage_candidate: false,
            crate_provenance: CrateProvenance::LegacyUnknown,
            crate_id: 0,
            complete: true,
        }
    }
}

impl FnNode {
    pub(crate) fn rvs_dependency_calls(&self) -> impl Iterator<Item = &DefPath> {
        self.calls
            .keys()
            .map(|identity| &identity.def_path)
            .chain(self.entry_calls.keys())
    }
}

/// Merge another callgraph entry for the same function into `target`.
///
/// A free function instead of an `&mut self` method: `FnNode` is pure data
/// with all fields visible, and mutator methods on pure-data structs are
/// rejected by the data-structure lint.
pub fn rvs_merge_fn_node_M(target: &mut FnNode, other: &FnNode) {
    for (identity, edge_type) in &other.calls {
        rvs_merge_identity_call_edge_M(&mut target.calls, identity.clone(), *edge_type);
    }
    for (def_path, edge_type) in &other.entry_calls {
        rvs_merge_call_edge_M(&mut target.entry_calls, def_path.clone(), *edge_type);
    }
    target
        .unresolved_test_calls
        .extend(other.unresolved_test_calls.iter().cloned());
    rvs_merge_capability_facts_M(&mut target.facts, other.facts);
    target.has_body |= other.has_body;
    target.is_trait_impl |= other.is_trait_impl;
    target.is_test |= other.is_test;
    target.is_entrypoint |= other.is_entrypoint;
    target.is_test_compilation |= other.is_test_compilation;
    target.call_sites.extend(other.call_sites.iter().cloned());
    target.sources.extend(other.sources.iter().cloned());
    target.report_line_count = target.report_line_count.max(other.report_line_count);
    target.report_function_count = target
        .report_function_count
        .max(other.report_function_count);
    target.allows_dead_code |= other.allows_dead_code;
    target.is_production |= other.is_production;
    target.is_coverage_candidate |= other.is_coverage_candidate;
    target.complete &= other.complete;
}

/// Retarget a test-only node copy at `crate_id`.
#[cfg(test)]
pub(crate) fn rvs_test_target_of_M(node: &mut FnNode, crate_id: u64) -> &mut FnNode {
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    node.crate_id = crate_id;
    node
}

fn rvs_merge_identity_call_edge_M(
    calls: &mut BTreeMap<FunctionIdentity, CallEdgeType>,
    identity: FunctionIdentity,
    new_edge: CallEdgeType,
) {
    match calls.get(&identity) {
        Some(CallEdgeType::Strong) => {}
        Some(CallEdgeType::Weak) | None => {
            calls.insert(identity, new_edge);
        }
    }
}

fn rvs_merge_call_edge_M(
    calls: &mut BTreeMap<DefPath, CallEdgeType>,
    def_path: DefPath,
    new_edge: CallEdgeType,
) {
    match calls.get(&def_path) {
        Some(CallEdgeType::Strong) => {}
        Some(CallEdgeType::Weak) => {
            calls.insert(def_path, new_edge);
        }
        None => {
            calls.insert(def_path, new_edge);
        }
    }
}

const fn rvs_is_false(value: &bool) -> bool {
    !*value
}

const fn rvs_is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone)]
pub struct FnGraph {
    pub nodes: BTreeMap<DefPath, FnNode>,
    format: CallgraphFormat,
}

impl std::fmt::Debug for FnGraph {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FnGraph")
            .field("nodes", &self.nodes)
            .finish()
    }
}

impl Default for FnGraph {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            format: CallgraphFormat::Current,
        }
    }
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
        for node in self.nodes.values().filter(|node| node.is_test) {
            pending.extend(
                node.calls
                    .iter()
                    .filter(|(_, edge)| **edge == CallEdgeType::Strong)
                    .map(|(identity, _)| identity.clone()),
            );
        }
        while let Some(identity) = pending.pop_front() {
            if !covered.insert(identity.clone()) {
                continue;
            }
            if let Some(node) = self.nodes.get(&identity.def_path) {
                pending.extend(
                    node.calls
                        .iter()
                        .filter(|(_, edge)| **edge == CallEdgeType::Strong)
                        .map(|(identity, _)| identity.clone()),
                );
            }
        }
        covered
    }

    #[cfg(test)]
    pub(crate) fn rvs_insert_M(&mut self, path: DefPath, node: FnNode) {
        self.nodes.insert(path, node);
    }

    pub(crate) fn rvs_merge_node_M(
        &mut self,
        path: &DefPath,
        node: &FnNode,
    ) -> Result<(), CallgraphArtifactError> {
        if let Some(existing) = self.nodes.get_mut(path) {
            rvs_merge_fn_node_M(existing, node);
        } else {
            self.nodes.insert(path.clone(), node.clone());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn rvs_merge_from_M(&mut self, other: &Self) -> Result<(), CallgraphArtifactError> {
        for (path, node) in &other.nodes {
            self.rvs_merge_node_M(path, node)?;
        }
        Ok(())
    }

    pub(crate) fn rvs_merge_artifacts(
        artifacts: Vec<Self>,
        local_crate_names: &BTreeSet<CrateName>,
    ) -> Result<Self, CallgraphArtifactError> {
        let _ = local_crate_names;
        let legacy_count = artifacts
            .iter()
            .filter(|artifact| artifact.format == CallgraphFormat::Legacy)
            .count();
        let current_count = artifacts.len().saturating_sub(legacy_count);
        if legacy_count > 0 && current_count > 0 {
            return Err(CallgraphArtifactError::MixedArtifactFormats {
                legacy_count,
                current_count,
            });
        }
        if legacy_count > 0 {
            let mut merged = Self {
                nodes: BTreeMap::new(),
                format: CallgraphFormat::Legacy,
            };
            for artifact in artifacts {
                for (path, mut node) in artifact.nodes {
                    node.complete = false;
                    merged.rvs_merge_node_M(&path, &node)?;
                }
            }
            return Ok(merged);
        }

        let mut merged = Self::rvs_new();
        let mut provenance_by_crate_id = BTreeMap::new();
        for artifact in artifacts {
            for (path, node) in artifact.nodes {
                rvs_assert_node_record(&path, &node)?;
                rvs_record_crate_provenance_M(
                    &mut provenance_by_crate_id,
                    node.crate_id,
                    node.crate_provenance,
                    &path,
                )?;
                merged.rvs_merge_node_M(&path, &node)?;
            }
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

    pub(crate) fn rvs_is_legacy(&self) -> bool {
        self.format == CallgraphFormat::Legacy
    }
}

/// Resolve a user-facing function query the way `cargo rivus why` does:
/// an exact graph or synthetic match wins; otherwise every graph and
/// synthetic path whose readable form equals the query matches.
pub(crate) fn rvs_function_query_matches(
    graph: &FnGraph,
    synthetic_paths: &BTreeSet<DefPath>,
    query: &str,
) -> Vec<DefPath> {
    let exact = DefPath::from(query);
    if graph.rvs_get(query).is_some() || synthetic_paths.contains(&exact) {
        return vec![exact];
    }
    graph
        .rvs_keys()
        .chain(synthetic_paths.iter())
        .filter(|path| path.rvs_user_path() == query)
        .cloned()
        .collect()
}

fn rvs_record_crate_provenance_M(
    provenance_by_crate_id: &mut BTreeMap<u64, (CrateProvenance, DefPath)>,
    crate_id: u64,
    provenance: CrateProvenance,
    path: &DefPath,
) -> Result<(), CallgraphArtifactError> {
    if crate_id == 0 {
        return Err(CallgraphArtifactError::ZeroCrateId {
            def_path: path.clone(),
        });
    }
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    if provenance == CrateProvenance::LegacyUnknown {
        return Err(CallgraphArtifactError::UnknownCrateProvenance {
            def_path: path.clone(),
            crate_id,
        });
    }
    match provenance_by_crate_id.get(&crate_id) {
        Some((existing, existing_path)) if *existing != provenance => {
            Err(CallgraphArtifactError::ConflictingCrateProvenance {
                crate_id,
                existing: *existing,
                existing_path: existing_path.clone(),
                incoming: provenance,
                incoming_path: path.clone(),
            })
        }
        Some(_) => Ok(()),
        None => {
            provenance_by_crate_id.insert(crate_id, (provenance, path.clone()));
            Ok(())
        }
    }
}

pub(crate) fn rvs_serialize_callgraph_json(
    graph: &FnGraph,
) -> Result<String, CallgraphArtifactError> {
    if graph.format == CallgraphFormat::Legacy {
        return Err(CallgraphArtifactError::LegacySerialization);
    }
    let mut provenance_by_crate_id = BTreeMap::new();
    let nodes = graph
        .nodes
        .iter()
        .map(|(path, node)| {
            rvs_assert_node_record(path, node)?;
            rvs_record_crate_provenance_M(
                &mut provenance_by_crate_id,
                node.crate_id,
                node.crate_provenance,
                path,
            )?;
            Ok((path.clone(), FnNodeArtifact::rvs_from_node(node)))
        })
        .collect::<Result<BTreeMap<_, _>, CallgraphArtifactError>>()?;
    let artifact = CallgraphArtifact {
        schema_version: CALLGRAPH_SCHEMA_VERSION,
        nodes,
    };
    serde_json::to_string(&artifact)
        .map_err(|source| CallgraphArtifactError::SerializeCallgraph { source })
}

/// Coverage class of an untested function, as classified by the offline
/// engine's semantic caps. The emission compile trusts this label instead
/// of reclassifying from signature facts, which cannot see propagated
/// capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoverageLabel {
    Good,
    Ok,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UntestedSelectionEntry {
    pub(crate) identity: FunctionIdentity,
    pub(crate) label: CoverageLabel,
}

pub(crate) fn rvs_serialize_untested_selection(
    selection: &BTreeMap<FunctionIdentity, CoverageLabel>,
) -> Result<String, CallgraphArtifactError> {
    let entries: Vec<UntestedSelectionEntry> = selection
        .iter()
        .map(|(identity, label)| UntestedSelectionEntry {
            identity: identity.clone(),
            label: *label,
        })
        .collect();
    serde_json::to_string(&entries)
        .map_err(|source| CallgraphArtifactError::SerializeFunctionIdentities { source })
}

pub(crate) fn rvs_parse_untested_selection(
    json: &str,
) -> Result<BTreeMap<FunctionIdentity, CoverageLabel>, CallgraphArtifactError> {
    let entries: Vec<UntestedSelectionEntry> = serde_json::from_str(json)
        .map_err(|source| CallgraphArtifactError::ParseFunctionIdentities { source })?;
    let mut selection = BTreeMap::new();
    for entry in entries {
        selection.insert(entry.identity, entry.label);
    }
    Ok(selection)
}

/// Parse versioned or legacy callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json(json: &str) -> Result<FnGraph, CallgraphArtifactError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|source| CallgraphArtifactError::InvalidJson { source })?;
    let object = value
        .as_object()
        .ok_or(CallgraphArtifactError::RootMustBeObject)?;
    let is_versioned = object.contains_key("schema_version") || object.contains_key("nodes");
    let graph = if is_versioned {
        let schema_version = object
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(CallgraphArtifactError::InvalidSchemaVersion)?;
        if schema_version != u64::from(CALLGRAPH_SCHEMA_VERSION) {
            return Err(CallgraphArtifactError::UnsupportedSchemaVersion {
                actual: schema_version,
                expected: CALLGRAPH_SCHEMA_VERSION,
            });
        }
        let artifact: CallgraphArtifact = serde_json::from_value(value)
            .map_err(|source| CallgraphArtifactError::InvalidVersionedRecord { source })?;
        debug_assert_eq!(artifact.schema_version, CALLGRAPH_SCHEMA_VERSION);
        let mut graph = FnGraph::rvs_new();
        for (path, node) in artifact.nodes {
            graph.nodes.insert(path.clone(), node.rvs_into_node());
        }
        graph
    } else {
        for (def_path, node) in object {
            if node
                .as_object()
                .is_some_and(|fields| !fields.contains_key("has_body"))
            {
                return Err(CallgraphArtifactError::StaleLegacyMissingHasBody {
                    def_path: DefPath::from(def_path.as_str()),
                });
            }
        }
        let nodes: BTreeMap<DefPath, LegacyFnNodeArtifact> = serde_json::from_str(json)
            .map_err(|source| CallgraphArtifactError::InvalidLegacyRecord { source })?;
        FnGraph {
            nodes: nodes
                .into_iter()
                .map(|(path, node)| (path, node.rvs_into_node()))
                .collect(),
            format: CallgraphFormat::Legacy,
        }
    };
    let mut provenance_by_crate_id = BTreeMap::new();
    for (path, node) in &graph.nodes {
        if path.rvs_as_str().is_empty() {
            return Err(CallgraphArtifactError::EmptyFunctionPath);
        }
        for callee in node.rvs_dependency_calls() {
            if callee.rvs_as_str().is_empty() {
                return Err(CallgraphArtifactError::EmptyCalleePath {
                    caller: path.clone(),
                });
            }
        }
        if is_versioned {
            if node.crate_id == 0 {
                return Err(CallgraphArtifactError::ZeroCrateId {
                    def_path: path.clone(),
                });
            }
            rvs_record_crate_provenance_M(
                &mut provenance_by_crate_id,
                node.crate_id,
                node.crate_provenance,
                path,
            )?;
            rvs_assert_node_record(path, node)?;
        }
        for source in &node.sources {
            rvs_validate_fn_source(path, source)?;
        }
    }
    Ok(graph)
}

fn rvs_validate_fn_source<'source>(
    def_path: &DefPath,
    source: &'source FnSource,
) -> Result<&'source FnSource, CallgraphArtifactError> {
    if source.file.as_os_str().is_empty() {
        return Err(CallgraphArtifactError::EmptySourceFile {
            def_path: def_path.clone(),
        });
    }
    if let Some(base) = &source.base {
        if base.as_os_str().is_empty() {
            return Err(CallgraphArtifactError::EmptySourceBase {
                def_path: def_path.clone(),
            });
        }
        if source.file.is_absolute() {
            return Err(CallgraphArtifactError::AbsoluteSourceWithBase {
                def_path: def_path.clone(),
            });
        }
        if !base.is_absolute() {
            return Err(CallgraphArtifactError::RelativeSourceBase {
                def_path: def_path.clone(),
            });
        }
    }
    if source.name_start >= source.name_end {
        return Err(CallgraphArtifactError::InvalidSourceRange {
            def_path: def_path.clone(),
            start: source.name_start,
            end: source.name_end,
        });
    }
    Ok(source)
}

fn rvs_assert_node_record(caller: &DefPath, node: &FnNode) -> Result<(), CallgraphArtifactError> {
    for source in &node.sources {
        rvs_validate_fn_source(caller, source)?;
    }
    if let Some(call) = node.calls.keys().find(|call| call.crate_id == 0) {
        return Err(CallgraphArtifactError::ZeroCalleeCrateId {
            caller: caller.clone(),
            callee: call.def_path.clone(),
        });
    }
    let mut occurrences = BTreeSet::new();
    for call_site in &node.call_sites {
        if call_site.callee.crate_id == 0 || call_site.callee.def_path.rvs_as_str().is_empty() {
            return Err(CallgraphArtifactError::InvalidCallSiteCallee {
                caller: caller.clone(),
                occurrence: call_site.occurrence,
            });
        }
        if !occurrences.insert(call_site.occurrence) {
            return Err(CallgraphArtifactError::RepeatedCallOccurrence {
                caller: caller.clone(),
                occurrence: call_site.occurrence,
            });
        }
        if let Some(source) = &call_site.source {
            rvs_validate_call_site_source(source, call_site, caller)?;
        }
    }
    let site_callees = node
        .call_sites
        .iter()
        .map(|call_site| call_site.callee.clone())
        .collect::<BTreeSet<_>>();
    let call_callees = node.calls.keys().cloned().collect::<BTreeSet<_>>();
    if call_callees != site_callees {
        return Err(CallgraphArtifactError::TargetCallsMismatch {
            caller: caller.clone(),
        });
    }
    Ok(())
}

fn rvs_validate_call_site_source<'source>(
    source: &'source CallSiteSource,
    call_site: &CallSiteIdentity,
    caller: &DefPath,
) -> Result<&'source CallSiteSource, CallgraphArtifactError> {
    let occurrence = call_site.occurrence;
    if source.file.as_os_str().is_empty() {
        return Err(CallgraphArtifactError::EmptyCallSiteSourceFile {
            caller: caller.clone(),
            occurrence,
        });
    }
    if let Some(base) = &source.base {
        if base.as_os_str().is_empty() {
            return Err(CallgraphArtifactError::EmptyCallSiteSourceBase {
                caller: caller.clone(),
                occurrence,
            });
        }
        if source.file.is_absolute() || !base.is_absolute() {
            return Err(CallgraphArtifactError::InconsistentCallSiteSourceBase {
                caller: caller.clone(),
                occurrence,
            });
        }
    }
    if source.start >= source.end {
        return Err(CallgraphArtifactError::InvalidCallSiteSourceRange {
            caller: caller.clone(),
            occurrence,
        });
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260730_validators_return_original_borrowed_values() {
        let caller = DefPath::from("demo::rvs_run");
        let source = FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 14);
        let validated_source = rvs_validate_fn_source(&caller, &source).unwrap();

        let callee = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("dependency::rvs_fetch_AI"),
        };
        let call_site_source = CallSiteSource::rvs_new(PathBuf::from("src/lib.rs"), 20, 31);
        let call_site = CallSiteIdentity {
            callee: callee.clone(),
            occurrence: 0,
            source: Some(call_site_source.clone()),
        };
        let mut node = FnNode {
            crate_id: 7,
            calls: BTreeMap::from([(callee, CallEdgeType::Strong)]),
            call_sites: BTreeSet::from([call_site]),
            sources: BTreeSet::from([source.clone()]),
            ..FnNode::default()
        };
        node.crate_provenance = CrateProvenance::PrimaryPackage;
        rvs_assert_node_record(&caller, &node).unwrap();

        let output = "validators accept well-formed flat node records\n";
        rvs_snapshot_BIS(
            "test_20260730_validators_return_original_borrowed_values",
            &output,
        );

        assert!(!node.calls.is_empty());
        assert!(validated_source.name_start < validated_source.name_end);
    }

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
        let result = rvs_parse_callgraph_json(json).unwrap();
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS("test_20260609_parse_callgraph_valid_json", &output);
        assert_eq!(result.rvs_len(), 2);
        assert!(rvs_serialize_callgraph_json(&result).is_err());
        assert_eq!(result.rvs_values().count(), 2);
    }

    #[test]
    fn test_20260729_artifact_merge_rejects_conflicting_crate_provenance() {
        let make_graph = |path: &str, provenance| {
            let mut node = FnNode::default();
            node.crate_id = 7;
            node.crate_provenance = provenance;
            let mut graph = FnGraph::rvs_new();
            graph.rvs_insert_M(DefPath::from(path), node);
            graph
        };
        let result = FnGraph::rvs_merge_artifacts(
            vec![
                make_graph("demo::rvs_local", CrateProvenance::PrimaryPackage),
                make_graph("demo::rvs_dependency", CrateProvenance::Dependency),
            ],
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260729_artifact_merge_rejects_conflicting_crate_provenance",
            &output,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_20260729_callgraph_wire_rejects_unknown_fields_at_every_level() {
        let callee = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("dependency::rvs_read_BI"),
        };
        let mut node = FnNode {
            crate_id: 7,
            calls: BTreeMap::from([(callee.clone(), CallEdgeType::Strong)]),
            call_sites: BTreeSet::from([CallSiteIdentity {
                callee,
                occurrence: 0,
                source: Some(CallSiteSource::rvs_new("src/lib.rs".into(), 20, 30)),
            }]),
            facts: CapabilityFacts {
                has_async: true,
                ..CapabilityFacts::default()
            },
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 4, 11)]),
            crate_provenance: CrateProvenance::PrimaryPackage,
            ..FnNode::default()
        };
        node.crate_id = 7;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_A"), node);
        let json = rvs_serialize_callgraph_json(&graph).unwrap();
        let base: serde_json::Value = serde_json::from_str(&json).unwrap();

        let mut cases = Vec::new();
        let mut top_level = base.clone();
        top_level["future_top_level"] = true.into();
        cases.push(("top_level", top_level));

        let mut node = base.clone();
        node["nodes"]["demo::rvs_run_A"]["future_node"] = true.into();
        cases.push(("node", node));

        let mut call_identity = base.clone();
        call_identity["nodes"]["demo::rvs_run_A"]["calls"][0]["future_identity"] = true.into();
        cases.push(("call_identity", call_identity));

        let mut call_site = base.clone();
        call_site["nodes"]["demo::rvs_run_A"]["call_sites"][0]["future_call_site"] = true.into();
        cases.push(("call_site", call_site));

        let mut call_site_source = base.clone();
        call_site_source["nodes"]["demo::rvs_run_A"]["call_sites"][0]["source"]["future_call_site_source"] =
            true.into();
        cases.push(("call_site_source", call_site_source));

        let mut fact = base.clone();
        fact["nodes"]["demo::rvs_run_A"]["facts"]["future_fact"] = true.into();
        cases.push(("fact", fact));

        let mut source = base.clone();
        source["nodes"]["demo::rvs_run_A"]["sources"][0]["future_source"] = true.into();
        cases.push(("source", source));

        let mut provenance = base;
        provenance["nodes"]["demo::rvs_run_A"]["crate_provenance"] =
            serde_json::from_str(r#"{"kind":"primary_package","future_provenance":true}"#).unwrap();
        cases.push(("provenance", provenance));

        let mut output = String::new();
        for (name, value) in cases {
            let accepted =
                rvs_parse_callgraph_json(&serde_json::to_string(&value).unwrap()).is_ok();
            output.push_str(&format!("{name}: accepted={accepted}\n"));
        }
        rvs_snapshot_BIS(
            "test_20260729_callgraph_wire_rejects_unknown_fields_at_every_level",
            &output,
        );

        assert!(!output.contains("accepted=true"));
    }

    #[test]
    fn test_20260729_legacy_callgraph_remains_uncertain_and_read_only() {
        let json = r#"{
            "demo::rvs_legacy_A": {
                "calls": [],
                "production_crate_ids": [7],
                "coverage_candidate_crate_ids": [7],
                "facts_by_crate": {"7": {"has_async": true}},
                "has_body_by_crate": {"7": true},
                "sources_by_crate": {"7": [{"file":"src/lib.rs","name_start":4,"name_end":16}]},
                "has_body": true,
                "has_async": true,
                "sources": [{"file":"src/lib.rs","name_start":4,"name_end":16}],
                "report_line_count": 3,
                "report_function_count": 1
            }
        }"#;
        let mut graph = rvs_parse_callgraph_json(json).unwrap();
        let node = graph.rvs_get("demo::rvs_legacy_A").unwrap();
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let report_candidate = crate::function_classification::FunctionClassification::rvs_new(
            &scope,
            &DefPath::from("demo::rvs_legacy_A"),
            node,
        )
        .rvs_is_report_candidate();
        let serialize_error = rvs_serialize_callgraph_json(&graph).is_err();
        let analysis = crate::inference::PreparedInference::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let inference_incomplete = analysis
            .rvs_incomplete_paths()
            .contains(&DefPath::from("demo::rvs_legacy_A"));
        let empty_legacy = rvs_parse_callgraph_json("{}").unwrap();
        let empty_serialize_error = rvs_serialize_callgraph_json(&empty_legacy).is_err();
        let output = format!(
            "serialize_error={serialize_error}\nempty_serialize_error={empty_serialize_error}\nreport_candidate={report_candidate}\ninference_incomplete={inference_incomplete}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_legacy_callgraph_remains_uncertain_and_read_only",
            &output,
        );

        assert!(serialize_error);
        assert!(empty_serialize_error);
        assert!(!report_candidate);
        assert!(inference_incomplete);
    }

    #[test]
    fn test_20260729_artifact_merge_rejects_legacy_current_mixture() {
        let legacy =
            rvs_parse_callgraph_json(r#"{"legacy::rvs_read":{"calls":[],"has_body":true}}"#)
                .unwrap();
        let mut current_node = FnNode::default();
        current_node.crate_id = 7;
        current_node.crate_provenance = CrateProvenance::PrimaryPackage;
        let mut current = FnGraph::rvs_new();
        current.rvs_insert_M(DefPath::from("demo::rvs_current"), current_node);

        let forward = FnGraph::rvs_merge_artifacts(
            vec![legacy.clone(), current.clone()],
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let reverse = FnGraph::rvs_merge_artifacts(
            vec![current, legacy],
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let output = format!(
            "forward_error={}\nreverse_error={}\n",
            forward.is_err(),
            reverse.is_err(),
        );
        rvs_snapshot_BIS(
            "test_20260729_artifact_merge_rejects_legacy_current_mixture",
            &output,
        );

        assert!(forward.is_err());
        assert!(reverse.is_err());
    }

    #[test]
    fn test_20260729_duplicate_call_occurrences_are_preserved_and_validated() {
        let callee = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("dependency::rvs_read_BI"),
        };
        let call_sites = BTreeSet::from([
            CallSiteIdentity {
                callee: callee.clone(),
                occurrence: 0,
                source: Some(CallSiteSource::rvs_new("src/lib.rs".into(), 20, 24)),
            },
            CallSiteIdentity {
                callee: callee.clone(),
                occurrence: 1,
                source: Some(CallSiteSource::rvs_new("src/lib.rs".into(), 40, 44)),
            },
        ]);
        let mut node = FnNode {
            crate_id: 7,
            calls: BTreeMap::from([(callee, CallEdgeType::Strong)]),
            call_sites,
            crate_provenance: CrateProvenance::PrimaryPackage,
            ..FnNode::default()
        };
        node.crate_id = 7;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let json = rvs_serialize_callgraph_json(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json(&json).unwrap();
        let call_site_count = parsed
            .rvs_get("demo::rvs_run")
            .map_or(0, |node| node.call_sites.len());

        let mut repeated_occurrence: serde_json::Value = serde_json::from_str(&json).unwrap();
        repeated_occurrence["nodes"]["demo::rvs_run"]["call_sites"][1]["occurrence"] = 0.into();
        let repeated_occurrence_error =
            rvs_parse_callgraph_json(&serde_json::to_string(&repeated_occurrence).unwrap())
                .is_err();

        let local = BTreeSet::from([CrateName::from("demo")]);
        let forward_graph =
            FnGraph::rvs_merge_artifacts(vec![graph.clone(), graph.clone()], &local);
        let reverse_graph = FnGraph::rvs_merge_artifacts(vec![graph.clone(), graph], &local);
        let aggregate_complete = forward_graph
            .as_ref()
            .ok()
            .and_then(|merged| merged.rvs_get("demo::rvs_run"))
            .is_some_and(|node| node.complete);
        let forward = forward_graph
            .as_ref()
            .ok()
            .and_then(|merged| rvs_serialize_callgraph_json(merged).ok());
        let reverse = reverse_graph
            .as_ref()
            .ok()
            .and_then(|merged| rvs_serialize_callgraph_json(merged).ok());
        let deterministic = forward.is_some() && forward == reverse;
        let output = format!(
            "call_site_count={call_site_count}\nrepeated_occurrence_error={repeated_occurrence_error}\naggregate_complete={aggregate_complete}\ndeterministic={deterministic}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_duplicate_call_occurrences_are_preserved_and_validated",
            &output,
        );

        assert_eq!(call_site_count, 2);
        assert!(repeated_occurrence_error);
        assert!(aggregate_complete);
        assert!(deterministic);
    }

    #[test]
    fn test_20260810_unified_occurrence_preserves_source_order() {
        let weak_callee = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("demo::rvs_ref_first"),
        };
        let strong_callee = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("demo::rvs_call_second"),
        };
        let call_sites = BTreeSet::from([
            CallSiteIdentity {
                callee: weak_callee.clone(),
                occurrence: 0,
                source: None,
            },
            CallSiteIdentity {
                callee: strong_callee.clone(),
                occurrence: 1,
                source: None,
            },
        ]);
        let calls = BTreeMap::from([
            (weak_callee, CallEdgeType::Weak),
            (strong_callee, CallEdgeType::Strong),
        ]);
        let mut node = FnNode {
            calls,
            call_sites,
            crate_provenance: CrateProvenance::PrimaryPackage,
            crate_id: 7,
            ..FnNode::default()
        };
        node.crate_id = 7;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_order"), node);
        let json = rvs_serialize_callgraph_json(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json(&json).unwrap();
        let target = parsed
            .rvs_get("demo::rvs_order")
            .expect("never: target exists");
        let weak_edge = target
            .calls
            .get(&FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("demo::rvs_ref_first"),
            })
            .copied();
        let strong_edge = target
            .calls
            .get(&FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("demo::rvs_call_second"),
            })
            .copied();
        let occurrence_of_weak = target
            .call_sites
            .iter()
            .find(|cs| cs.callee.def_path == DefPath::from("demo::rvs_ref_first"))
            .map(|cs| cs.occurrence);
        let occurrence_of_strong = target
            .call_sites
            .iter()
            .find(|cs| cs.callee.def_path == DefPath::from("demo::rvs_call_second"))
            .map(|cs| cs.occurrence);
        let output = format!(
            "weak_edge={weak_edge:?}\nstrong_edge={strong_edge:?}\noccurrence_of_weak={occurrence_of_weak:?}\noccurrence_of_strong={occurrence_of_strong:?}\n"
        );
        rvs_snapshot_BIS(
            "test_20260810_unified_occurrence_preserves_source_order",
            &output,
        );

        assert_eq!(weak_edge, Some(CallEdgeType::Weak));
        assert_eq!(strong_edge, Some(CallEdgeType::Strong));
        assert_eq!(occurrence_of_weak, Some(0));
        assert_eq!(occurrence_of_strong, Some(1));
    }

    #[test]
    fn test_20260729_malformed_and_incompatible_callgraph_inputs_return_errors() {
        let truncated = std::panic::catch_unwind(|| {
            rvs_parse_callgraph_json(r#"{"schema_version":12,"nodes":{"demo::rvs_run""#)
        });
        let incompatible = std::panic::catch_unwind(|| {
            rvs_parse_callgraph_json(r#"{"schema_version":11,"nodes":{}}"#)
        });
        let truncated_error = truncated.is_ok_and(|result| result.is_err());
        let incompatible_error = incompatible.is_ok_and(|result| result.is_err());
        let output =
            format!("truncated_error={truncated_error}\nincompatible_error={incompatible_error}\n");
        rvs_snapshot_BIS(
            "test_20260729_malformed_and_incompatible_callgraph_inputs_return_errors",
            &output,
        );

        assert!(truncated_error);
        assert!(incompatible_error);
    }

    #[test]
    fn test_20260710_callgraph_artifact_version_roundtrip() {
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.crate_provenance = CrateProvenance::PrimaryPackage;
        node.facts.has_const = true;
        node.calls.insert(
            FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("std::fs::read"),
            },
            CallEdgeType::Strong,
        );
        node.call_sites.insert(CallSiteIdentity {
            callee: FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("std::fs::read"),
            },
            occurrence: 0,
            source: None,
        });
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);

        let json = rvs_serialize_callgraph_json(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json(&json).unwrap();
        let previous_version = json.replacen(
            &format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#),
            r#""schema_version":3"#,
            1,
        );
        let previous_version_error = rvs_parse_callgraph_json(&previous_version).unwrap_err();
        // Schema 17 predates CapabilityFacts.has_const and must be rejected
        // with an explicit version error instead of a record parse failure.
        let schema_17 = json.replacen(
            &format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#),
            r#""schema_version":17"#,
            1,
        );
        let schema_17_error = rvs_parse_callgraph_json(&schema_17).unwrap_err();
        let version_marker = format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#);
        let output = format!(
            "schema_version={CALLGRAPH_SCHEMA_VERSION}\ncontains_version={}\nnodes={}\nprevious_version_error={previous_version_error}\nschema_17_error={schema_17_error}\n",
            json.contains(&version_marker),
            parsed.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_version_roundtrip",
            &output,
        );

        assert!(json.contains(r#""nodes""#));
        assert!(parsed.rvs_get("demo::rvs_run").is_some());
        // The const fact must survive a schema-18 serialize/parse roundtrip.
        assert!(
            parsed
                .rvs_get("demo::rvs_run")
                .is_some_and(|node| node.facts.has_const)
        );
        assert!(matches!(
            schema_17_error,
            CallgraphArtifactError::UnsupportedSchemaVersion { actual: 17, .. }
        ));
    }

    #[test]
    fn test_20260816_schema_18_requires_every_capability_fact() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.crate_provenance = CrateProvenance::PrimaryPackage;
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let json = rvs_serialize_callgraph_json(&graph).unwrap();
        let base: serde_json::Value = serde_json::from_str(&json).unwrap();

        let fact_fields = [
            "has_async",
            "has_const",
            "is_unsafe_fn",
            "has_mut_param",
            "has_static_ref",
            "has_static_mut_ref",
            "has_thread_local_ref",
            "is_port_method",
        ];
        let mut rejected = Vec::new();
        let mut output = String::new();
        for field in fact_fields {
            let mut missing = base.clone();
            missing["nodes"]["demo::rvs_run"]
                .as_object_mut()
                .expect("never: serialized node is an object")
                .remove(field);
            let is_rejected =
                rvs_parse_callgraph_json(&serde_json::to_string(&missing).unwrap()).is_err();
            rejected.push(is_rejected);
            output.push_str(&format!("{field}: rejected={is_rejected}\n"));
        }

        let legacy = rvs_parse_callgraph_json(
            r#"{"demo::rvs_legacy_A":{"calls":[],"has_body":true,"has_async":true}}"#,
        );
        let legacy_async = legacy
            .as_ref()
            .ok()
            .and_then(|graph| graph.rvs_get("demo::rvs_legacy_A"))
            .is_some_and(|node| node.facts.has_async);
        let legacy_static_mut = legacy
            .as_ref()
            .ok()
            .and_then(|graph| graph.rvs_get("demo::rvs_legacy_A"))
            .is_some_and(|node| node.facts.has_static_mut_ref);
        output.push_str(&format!(
            "legacy_accepted={}\nlegacy_async={legacy_async}\nlegacy_static_mut={legacy_static_mut}\n",
            legacy.is_ok(),
        ));
        rvs_snapshot_BIS(
            "test_20260816_schema_18_requires_every_capability_fact",
            &output,
        );

        assert!(rejected.into_iter().all(|is_rejected| is_rejected));
        assert!(legacy.is_ok());
        assert!(legacy_async);
        assert!(!legacy_static_mut);
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
            let result = rvs_parse_callgraph_json(json);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_schema_validation",
            &output,
        );

        assert!(output.contains("UnsupportedSchemaVersion"));
        assert!(output.contains("InvalidSchemaVersion"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_validates_call_site_coverage_consistency() {
        let first = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("dependency::first"),
        };
        let second = FunctionIdentity {
            crate_id: 10,
            def_path: DefPath::from("dependency::second"),
        };
        let mut node = FnNode {
            calls: BTreeMap::from([
                (first.clone(), CallEdgeType::Strong),
                (second.clone(), CallEdgeType::Strong),
            ]),
            call_sites: BTreeSet::from([
                CallSiteIdentity {
                    callee: first,
                    occurrence: 0,
                    source: None,
                },
                CallSiteIdentity {
                    callee: second,
                    occurrence: 1,
                    source: None,
                },
            ]),
            is_production: true,
            crate_provenance: CrateProvenance::PrimaryPackage,
            crate_id: 7,
            ..FnNode::default()
        };
        node.crate_id = 7;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let valid_json = rvs_serialize_callgraph_json(&graph).unwrap();
        let mut missing_site_json: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        missing_site_json["nodes"]["demo::rvs_run"]["call_sites"]
            .as_array_mut()
            .expect("never: serialized call sites are an array")
            .pop();
        let missing_site =
            rvs_parse_callgraph_json(&serde_json::to_string(&missing_site_json).unwrap())
                .unwrap_err();
        let mut duplicate_occurrence_json: serde_json::Value =
            serde_json::from_str(&valid_json).unwrap();
        duplicate_occurrence_json["nodes"]["demo::rvs_run"]["call_sites"][1]["occurrence"] =
            0.into();
        let duplicate_occurrence =
            rvs_parse_callgraph_json(&serde_json::to_string(&duplicate_occurrence_json).unwrap())
                .unwrap_err();
        let valid = rvs_parse_callgraph_json(&valid_json);
        let output = format!(
            "missing_site={missing_site}\nduplicate_occurrence={duplicate_occurrence}\nvalid={}\n",
            valid.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_validates_call_site_coverage_consistency",
            &output,
        );

        assert!(missing_site.to_string().contains("call sites"));
        assert!(duplicate_occurrence.to_string().contains("occurrence"));
        assert!(valid.is_ok());
    }

    #[test]
    fn test_20260714_def_path_selection_roundtrip() {
        let selection = BTreeMap::from([
            (
                FunctionIdentity {
                    crate_id: 7,
                    def_path: DefPath::from("demo::rvs_alpha"),
                },
                CoverageLabel::Good,
            ),
            (
                FunctionIdentity {
                    crate_id: 9,
                    def_path: DefPath::from("demo::Worker::rvs_run_P"),
                },
                CoverageLabel::Ok,
            ),
        ]);

        let json = rvs_serialize_untested_selection(&selection).unwrap();
        let parsed = rvs_parse_untested_selection(&json).unwrap();
        let output = format!("json={json}\nparsed={parsed:?}\n");
        rvs_snapshot_BIS("test_20260714_def_path_selection_roundtrip", &output);

        assert_eq!(parsed, selection);
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
        base.rvs_merge_from_M(&extended).unwrap();

        let mut merged = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.calls.insert(
            FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("std::fs::read_to_string"),
            },
            CallEdgeType::Strong,
        );
        merged.rvs_insert_M(DefPath::from("demo::rvs_a"), node);
        base.rvs_merge_from_M(&merged).unwrap();

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
    fn test_20260706_graph_merge_node_preserves_body_and_sources() {
        let mut graph = FnGraph::rvs_new();
        let path = DefPath::from("demo::rvs_run");
        let mut bodyless = FnNode {
            has_body: false,
            ..FnNode::default()
        };
        bodyless.facts.has_async = true;
        graph.rvs_merge_node_M(&path, &bodyless).unwrap();

        let mut with_body = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 14)]),
            ..FnNode::default()
        };
        with_body.calls.insert(
            FunctionIdentity {
                crate_id: 9,
                def_path: DefPath::from("dep::rvs_call_BI"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_merge_node_M(&path, &with_body).unwrap();

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
        let legacy = rvs_parse_callgraph_json(legacy_json).unwrap();
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
            let result = rvs_parse_callgraph_json(&json);
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
        ];
        let mut output = String::new();
        for (name, json) in cases {
            let result = rvs_parse_callgraph_json(json);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS("test_20260709_parse_callgraph_rejection_table", &output);
    }

    #[test]
    fn test_20260811_weak_edge_does_not_provide_test_coverage() {
        let strong_target = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_strong_callee"),
        };
        let weak_target = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_weak_callee"),
        };
        let strong_leaf = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_strong_leaf"),
        };
        let weak_leaf = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_weak_leaf"),
        };
        let hidden_behind_weak_root = FunctionIdentity {
            crate_id: 1,
            def_path: DefPath::from("demo::rvs_hidden_behind_weak_root"),
        };
        let mut test_node = FnNode {
            is_test: true,
            crate_id: 1,
            ..FnNode::default()
        };
        test_node.calls = BTreeMap::from([
            (strong_target.clone(), CallEdgeType::Strong),
            (weak_target.clone(), CallEdgeType::Weak),
        ]);
        let mut strong_node = FnNode {
            crate_id: 1,
            ..FnNode::default()
        };
        strong_node.calls = BTreeMap::from([
            (strong_leaf.clone(), CallEdgeType::Strong),
            (weak_leaf.clone(), CallEdgeType::Weak),
        ]);
        let mut weak_node = FnNode {
            crate_id: 1,
            ..FnNode::default()
        };
        weak_node.calls = BTreeMap::from([(hidden_behind_weak_root.clone(), CallEdgeType::Strong)]);
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::test_fn"), test_node);
        graph.rvs_insert_M(strong_target.def_path.clone(), strong_node);
        graph.rvs_insert_M(weak_target.def_path.clone(), weak_node);
        let covered = graph.rvs_test_reachable_identities();
        let strong_covered = covered.contains(&strong_target);
        let weak_covered = covered.contains(&weak_target);
        let strong_leaf_covered = covered.contains(&strong_leaf);
        let weak_leaf_covered = covered.contains(&weak_leaf);
        let hidden_behind_weak_root_covered = covered.contains(&hidden_behind_weak_root);
        let output = format!(
            "strong_covered={strong_covered}\nweak_covered={weak_covered}\nstrong_leaf_covered={strong_leaf_covered}\nweak_leaf_covered={weak_leaf_covered}\nhidden_behind_weak_root_covered={hidden_behind_weak_root_covered}\n"
        );
        rvs_snapshot_BIS(
            "test_20260811_weak_edge_does_not_provide_test_coverage",
            &output,
        );

        assert!(strong_covered);
        assert!(!weak_covered);
        assert!(strong_leaf_covered);
        assert!(!weak_leaf_covered);
        assert!(!hidden_behind_weak_root_covered);
    }
}
