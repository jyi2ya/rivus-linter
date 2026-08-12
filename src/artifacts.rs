use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::capability::CapabilityFacts;
use crate::function_classification::LocalScope;
use crate::symbols::{CrateName, DefPath};

pub(crate) const CALLGRAPH_SCHEMA_VERSION: u32 = 16;

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
    #[snafu(display(
        "invalid callgraph artifact: targets is required for {def_path} in schema version {schema_version}"
    ))]
    MissingTargets {
        def_path: DefPath,
        schema_version: u32,
    },
    #[snafu(display("invalid callgraph artifact: node for {def_path} has no target identity"))]
    MissingTargetIdentity { def_path: DefPath },
    #[snafu(display("cannot serialize a headerless legacy callgraph as current schema truth"))]
    LegacySerialization,
    #[snafu(display(
        "cannot serialize callgraph artifact: node for {def_path} has no target identity"
    ))]
    SerializeMissingTargetIdentity { def_path: DefPath },
    #[snafu(display(
        "callgraph target record conflict for {def_path} crate id {crate_id}; asymmetric fields: {fields}"
    ))]
    TargetRecordConflict {
        def_path: DefPath,
        crate_id: u64,
        fields: String,
    },
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
        "cannot merge callgraph artifact: function {def_path} mixes legacy aggregate data with target records"
    ))]
    MixedNodeFormats { def_path: DefPath },
    #[snafu(display(
        "function {def_path} is both an executable entry point and an ordinary function at the same source location"
    ))]
    EntrypointSourceConflict { def_path: DefPath },
    #[snafu(display("function {def_path} has incompatible roles across Cargo targets"))]
    IncompatibleTargetRoles { def_path: DefPath },
    #[snafu(display("{counter} overflow while rebuilding {def_path}"))]
    AggregateCounterOverflow {
        def_path: DefPath,
        counter: &'static str,
    },
    #[snafu(display("invalid callgraph JSON: function path is empty"))]
    EmptyFunctionPath,
    #[snafu(display("invalid callgraph JSON: callee path for {caller} is empty"))]
    EmptyCalleePath { caller: DefPath },
    #[snafu(display(
        "invalid callgraph artifact: target metadata for {def_path} contains zero crate id"
    ))]
    ZeroTargetCrateId { def_path: DefPath },
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
        "invalid callgraph artifact: call_sites for {caller} crate id {crate_id} repeats occurrence {occurrence}"
    ))]
    RepeatedCallOccurrence {
        caller: DefPath,
        crate_id: u64,
        occurrence: u32,
    },
    #[snafu(display(
        "invalid callgraph artifact: target call sites for {caller} crate id {crate_id} do not match target calls"
    ))]
    TargetCallsMismatch { caller: DefPath, crate_id: u64 },
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FnNodeArtifact {
    targets: BTreeMap<u64, FnTargetData>,
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FnTargetData {
    #[serde(
        serialize_with = "rvs_serialize_call_edges_S",
        deserialize_with = "rvs_deserialize_call_edges_S"
    )]
    pub(crate) calls: BTreeMap<FunctionIdentity, CallEdgeType>,
    pub(crate) call_sites: BTreeSet<CallSiteIdentity>,
    pub(crate) unresolved_test_calls: BTreeSet<String>,
    pub(crate) facts: CapabilityFacts,
    pub(crate) has_body: bool,
    pub(crate) is_trait_impl: bool,
    pub(crate) is_test: bool,
    pub(crate) is_entrypoint: bool,
    pub(crate) is_test_compilation: bool,
    pub(crate) sources: BTreeSet<FnSource>,
    pub(crate) report_line_count: Option<usize>,
    pub(crate) report_function_count: usize,
    pub(crate) allows_dead_code: bool,
    pub(crate) is_production: bool,
    pub(crate) is_coverage_candidate: bool,
    pub(crate) crate_provenance: CrateProvenance,
}

impl Default for FnTargetData {
    fn default() -> Self {
        Self {
            calls: BTreeMap::new(),
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
            crate_provenance: CrateProvenance::PrimaryPackage,
        }
    }
}

impl FnTargetData {
    fn rvs_asymmetric_fields(&self, other: &Self) -> String {
        let fields = [
            (self.calls != other.calls, "calls"),
            (self.call_sites != other.call_sites, "call_sites"),
            (
                self.unresolved_test_calls != other.unresolved_test_calls,
                "unresolved_test_calls",
            ),
            (self.facts != other.facts, "facts"),
            (self.has_body != other.has_body, "has_body"),
            (self.is_trait_impl != other.is_trait_impl, "is_trait_impl"),
            (self.is_test != other.is_test, "is_test"),
            (self.is_entrypoint != other.is_entrypoint, "is_entrypoint"),
            (
                self.is_test_compilation != other.is_test_compilation,
                "is_test_compilation",
            ),
            (self.sources != other.sources, "sources"),
            (
                self.report_line_count != other.report_line_count,
                "report_line_count",
            ),
            (
                self.report_function_count != other.report_function_count,
                "report_function_count",
            ),
            (
                self.allows_dead_code != other.allows_dead_code,
                "allows_dead_code",
            ),
            (self.is_production != other.is_production, "is_production"),
            (
                self.is_coverage_candidate != other.is_coverage_candidate,
                "is_coverage_candidate",
            ),
            (
                self.crate_provenance != other.crate_provenance,
                "crate_provenance",
            ),
        ]
        .into_iter()
        .filter_map(|(different, field)| different.then_some(field))
        .collect::<Vec<_>>();
        debug_assert!(
            !fields.is_empty(),
            "unequal target records differ by a field"
        );
        fields.join(", ")
    }
}

fn rvs_serialize_call_edges_S<S>(
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

fn rvs_deserialize_call_edges_S<'de, D>(
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
    fn rvs_into_node_M(self, path: &DefPath) -> Result<FnNode, CallgraphArtifactError> {
        if self.targets.is_empty() {
            return Err(CallgraphArtifactError::MissingTargetIdentity {
                def_path: path.clone(),
            });
        }
        let mut node = FnNode {
            targets: self.targets,
            ..FnNode::default()
        };
        node.rvs_rebuild_from_targets_M(path)?;
        Ok(node)
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
                .map(|path| (path, CallEdgeType::Strong))
                .collect(),
            entry_calls: self
                .entry_calls
                .into_iter()
                .map(|path| (path, CallEdgeType::Strong))
                .collect(),
            unresolved_test_calls: self.unresolved_test_calls,
            targets: BTreeMap::new(),
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
    pub calls: BTreeMap<DefPath, CallEdgeType>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entry_calls: BTreeMap<DefPath, CallEdgeType>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unresolved_test_calls: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) targets: BTreeMap<u64, FnTargetData>,
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
    #[serde(skip)]
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Copy)]
/// Identity-bound view of a function target.
///
/// Accessors read the selected target record exclusively. The node aggregate is consulted only
/// when the node has no target records at all, which supports unversioned legacy graphs.
pub(crate) struct FnTarget<'a> {
    node: &'a FnNode,
    crate_id: u64,
}

impl<'a> FnTarget<'a> {
    fn rvs_data(self) -> Option<&'a FnTargetData> {
        self.node.targets.get(&self.crate_id)
    }

    fn rvs_uses_legacy_aggregate(self) -> bool {
        self.node.targets.is_empty()
    }

    pub(crate) fn rvs_exists(self) -> bool {
        self.rvs_uses_legacy_aggregate() || self.rvs_data().is_some()
    }

    pub(crate) fn rvs_facts(self) -> CapabilityFacts {
        if let Some(target) = self.rvs_data() {
            return target.facts;
        }
        if self.rvs_uses_legacy_aggregate() {
            self.node.facts
        } else {
            CapabilityFacts::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn rvs_has_body(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return target.has_body;
        }
        self.rvs_uses_legacy_aggregate() && self.node.has_body
    }

    pub(crate) fn rvs_is_entrypoint(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return target.is_entrypoint;
        }
        self.rvs_uses_legacy_aggregate() && self.node.is_entrypoint
    }

    pub(crate) fn rvs_is_test(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return target.is_test;
        }
        self.rvs_uses_legacy_aggregate() && self.node.is_test
    }

    pub(crate) fn rvs_is_trait_impl(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return target.is_trait_impl;
        }
        self.rvs_uses_legacy_aggregate() && self.node.is_trait_impl
    }

    #[cfg(test)]
    pub(crate) fn rvs_is_production(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return target.is_production;
        }
        false
    }

    pub(crate) fn rvs_crate_provenance(self) -> CrateProvenance {
        self.rvs_data()
            .map_or(CrateProvenance::LegacyUnknown, |target| {
                target.crate_provenance
            })
    }

    pub(crate) fn rvs_has_source(self) -> bool {
        if let Some(target) = self.rvs_data() {
            return !target.sources.is_empty();
        }
        self.rvs_uses_legacy_aggregate() && !self.node.sources.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn rvs_source_count(self) -> usize {
        if let Some(target) = self.rvs_data() {
            return target.sources.len();
        }
        if self.rvs_uses_legacy_aggregate() {
            self.node.sources.len()
        } else {
            0
        }
    }
}

impl Default for FnNode {
    fn default() -> Self {
        Self {
            calls: BTreeMap::new(),
            entry_calls: BTreeMap::new(),
            unresolved_test_calls: BTreeSet::new(),
            targets: BTreeMap::new(),
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
            complete: true,
        }
    }
}

impl FnNode {
    pub(crate) fn rvs_target_crate_ids(&self) -> BTreeSet<u64> {
        self.targets.keys().copied().collect()
    }

    #[cfg(test)]
    pub(crate) fn rvs_diagnostic_crate_ids(&self) -> BTreeSet<u64> {
        self.targets.keys().copied().collect()
    }

    pub(crate) fn rvs_target(&self, crate_id: u64) -> FnTarget<'_> {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        FnTarget {
            node: self,
            crate_id,
        }
    }

    pub(crate) fn rvs_insert_target_M(&mut self, crate_id: u64, target: FnTargetData) {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        match self.targets.entry(crate_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(target);
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                debug_assert_eq!(
                    entry.get(),
                    &target,
                    "one stable target identity has one complete record"
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn rvs_test_target_M(&mut self, crate_id: u64) -> &mut FnTargetData {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        self.targets.entry(crate_id).or_default()
    }

    #[cfg(test)]
    pub(crate) fn rvs_test_capture_target_M(
        &mut self,
        crate_id: u64,
        is_production: bool,
        is_coverage_candidate: bool,
    ) {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        let calls: BTreeMap<FunctionIdentity, CallEdgeType> = self
            .rvs_dependency_calls()
            .cloned()
            .map(|def_path| {
                (
                    FunctionIdentity { crate_id, def_path },
                    CallEdgeType::Strong,
                )
            })
            .collect();
        let call_sites = calls
            .keys()
            .enumerate()
            .map(|(occurrence, callee)| CallSiteIdentity {
                callee: callee.clone(),
                occurrence: u32::try_from(occurrence)
                    .expect("never: test node has at most u32::MAX callees"),
                source: None,
            })
            .collect();
        self.rvs_insert_target_M(
            crate_id,
            FnTargetData {
                calls,
                call_sites,
                unresolved_test_calls: self.unresolved_test_calls.clone(),
                facts: self.facts,
                has_body: self.has_body,
                is_trait_impl: self.is_trait_impl,
                is_test: self.is_test,
                is_entrypoint: self.is_entrypoint,
                is_test_compilation: self.is_test_compilation,
                sources: self.sources.clone(),
                report_line_count: self.report_line_count,
                report_function_count: self.report_function_count,
                allows_dead_code: self.allows_dead_code,
                is_production,
                is_coverage_candidate,
                crate_provenance: CrateProvenance::PrimaryPackage,
            },
        );
    }

    fn rvs_materialized_targets(&self) -> BTreeMap<u64, FnTargetData> {
        self.targets.clone()
    }

    fn rvs_rebuild_from_targets_M(&mut self, path: &DefPath) -> Result<(), CallgraphArtifactError> {
        if self.targets.is_empty() {
            return Ok(());
        }

        let all_targets = self.targets.values().collect::<Vec<_>>();
        let has_production_variant = all_targets.iter().any(|target| !target.is_test_compilation);
        let non_test_sources = all_targets
            .iter()
            .filter(|target| !target.is_test_compilation)
            .flat_map(|target| target.sources.iter().cloned())
            .collect::<BTreeSet<_>>();
        let retained = all_targets
            .iter()
            .copied()
            .filter(|target| {
                !target.is_test_compilation
                    || (!target.sources.is_empty()
                        && target
                            .sources
                            .iter()
                            .all(|source| !non_test_sources.contains(source)))
                    || (target.sources.is_empty() && !has_production_variant)
            })
            .collect::<Vec<_>>();
        let has_ordinary = retained.iter().any(|target| !target.is_entrypoint);
        let selected = retained
            .iter()
            .copied()
            .filter(|target| !has_ordinary || !target.is_entrypoint)
            .collect::<Vec<&FnTargetData>>();
        let entries = retained
            .iter()
            .copied()
            .filter(|target| target.is_entrypoint)
            .collect::<Vec<_>>();

        self.calls.clear();
        self.entry_calls.clear();
        self.unresolved_test_calls.clear();
        self.facts = CapabilityFacts::default();
        self.has_body = false;
        self.is_trait_impl = false;
        self.is_test = false;
        self.is_entrypoint =
            !selected.is_empty() && selected.iter().all(|target| target.is_entrypoint);
        self.is_test_compilation =
            !selected.is_empty() && selected.iter().all(|target| target.is_test_compilation);
        self.sources.clear();
        self.report_line_count = None;
        self.report_function_count = 0;
        self.allows_dead_code = false;
        self.complete = !all_targets.is_empty();

        for &target in &selected {
            for (identity, edge_type) in &target.calls {
                rvs_merge_call_edge_M(&mut self.calls, identity.def_path.clone(), *edge_type);
            }
        }
        if has_ordinary {
            for target in entries {
                for (identity, edge_type) in &target.calls {
                    rvs_merge_call_edge_M(
                        &mut self.entry_calls,
                        identity.def_path.clone(),
                        *edge_type,
                    );
                }
            }
        }

        for &target in &selected {
            self.unresolved_test_calls
                .extend(target.unresolved_test_calls.iter().cloned());
            self.facts.rvs_merge_M(target.facts);
            self.has_body |= target.has_body;
            self.is_trait_impl |= target.is_trait_impl;
            self.is_test |= target.is_test;
            self.sources.extend(target.sources.iter().cloned());
            self.allows_dead_code |= target.allows_dead_code;
        }

        let mut report_groups: Vec<Vec<&FnTargetData>> = Vec::new();
        for &target in &selected {
            let matching_groups = report_groups
                .iter()
                .enumerate()
                .filter_map(|(index, group)| {
                    group
                        .iter()
                        .any(|existing| {
                            (existing.sources.is_empty() && target.sources.is_empty())
                                || !existing.sources.is_disjoint(&target.sources)
                        })
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            if let Some(first) = matching_groups.first().copied() {
                let mut merged_group = vec![target];
                for index in matching_groups.into_iter().rev() {
                    merged_group.extend(report_groups.remove(index));
                }
                debug_assert!(first <= report_groups.len());
                report_groups.insert(first, merged_group);
            } else {
                report_groups.push(vec![target]);
            }
        }
        for group in report_groups {
            let line_count = group
                .iter()
                .filter_map(|target| target.report_line_count)
                .max();
            let function_count = group
                .iter()
                .map(|target| {
                    target
                        .report_function_count
                        .max(usize::from(target.report_line_count.is_some()))
                })
                .max()
                .unwrap_or(0);
            self.report_line_count = match (self.report_line_count, line_count) {
                (Some(left), Some(right)) => Some(left.checked_add(right).ok_or_else(|| {
                    CallgraphArtifactError::AggregateCounterOverflow {
                        def_path: path.clone(),
                        counter: "report line count",
                    }
                })?),
                (left, right) => left.or(right),
            };
            self.report_function_count = self
                .report_function_count
                .checked_add(function_count)
                .ok_or_else(|| CallgraphArtifactError::AggregateCounterOverflow {
                    def_path: path.clone(),
                    counter: "report function count",
                })?;
        }
        Ok(())
    }

    fn rvs_merge_coverage_M(
        &mut self,
        path: &DefPath,
        other: &Self,
    ) -> Result<(), CallgraphArtifactError> {
        for (crate_id, target) in &other.targets {
            match self.targets.entry(*crate_id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(target.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry) if entry.get() != target => {
                    return Err(CallgraphArtifactError::TargetRecordConflict {
                        def_path: path.clone(),
                        crate_id: *crate_id,
                        fields: entry.get().rvs_asymmetric_fields(target),
                    });
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
        Ok(())
    }

    /// Merge another callgraph entry for the same function into this one.
    pub fn rvs_merge_M(&mut self, other: &Self) {
        for (def_path, edge_type) in &other.calls {
            rvs_merge_call_edge_M(&mut self.calls, def_path.clone(), *edge_type);
        }
        for (def_path, edge_type) in &other.entry_calls {
            rvs_merge_call_edge_M(&mut self.entry_calls, def_path.clone(), *edge_type);
        }
        self.unresolved_test_calls
            .extend(other.unresolved_test_calls.iter().cloned());
        debug_assert!(
            self.targets.is_empty() && other.targets.is_empty(),
            "aggregate merge is reserved for identity-less in-memory compatibility nodes"
        );
        self.facts.rvs_merge_M(other.facts);
        self.has_body |= other.has_body;
        self.is_trait_impl |= other.is_trait_impl;
        self.is_test |= other.is_test;
        self.is_entrypoint |= other.is_entrypoint;
        self.is_test_compilation |= other.is_test_compilation;
        self.sources.extend(other.sources.iter().cloned());
        self.report_line_count = self.report_line_count.max(other.report_line_count);
        self.report_function_count = self.report_function_count.max(other.report_function_count);
        self.allows_dead_code |= other.allows_dead_code;
    }

    pub(crate) fn rvs_dependency_calls(&self) -> impl Iterator<Item = &DefPath> {
        self.calls.keys().chain(self.entry_calls.keys())
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

fn rvs_is_false(value: &bool) -> bool {
    !*value
}

fn rvs_is_zero(value: &usize) -> bool {
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
        for node in self.nodes.values() {
            for target in node.targets.values().filter(|target| target.is_test) {
                pending.extend(
                    target
                        .calls
                        .iter()
                        .filter(|(_, edge)| **edge == CallEdgeType::Strong)
                        .map(|(identity, _)| identity.clone()),
                );
            }
        }
        while let Some(identity) = pending.pop_front() {
            if !covered.insert(identity.clone()) {
                continue;
            }
            if let Some(node) = self.nodes.get(&identity.def_path) {
                pending.extend(node.targets.get(&identity.crate_id).into_iter().flat_map(
                    |target| {
                        target
                            .calls
                            .iter()
                            .filter(|(_, edge)| **edge == CallEdgeType::Strong)
                            .map(|(identity, _)| identity.clone())
                    },
                ));
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
            if existing.targets.is_empty() && node.targets.is_empty() {
                existing.rvs_merge_M(node);
            } else if existing.targets.is_empty() || node.targets.is_empty() {
                return Err(CallgraphArtifactError::MixedNodeFormats {
                    def_path: path.clone(),
                });
            } else {
                existing.rvs_merge_coverage_M(path, node)?;
                existing.rvs_rebuild_from_targets_M(path)?;
            }
        } else {
            let mut node = node.clone();
            if !node.targets.is_empty() {
                node.rvs_rebuild_from_targets_M(path)?;
            }
            self.nodes.insert(path.clone(), node);
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

        let mut variants: BTreeMap<DefPath, BTreeMap<u64, FnTargetData>> = BTreeMap::new();
        let mut provenance_by_crate_id = BTreeMap::new();
        for artifact in artifacts {
            for (path, node) in artifact.nodes {
                if node.targets.is_empty() {
                    return Err(CallgraphArtifactError::MixedNodeFormats { def_path: path });
                }
                for (crate_id, target) in &node.targets {
                    rvs_validate_target_record(&path, *crate_id, target)?;
                    rvs_record_crate_provenance_M(
                        &mut provenance_by_crate_id,
                        *crate_id,
                        target.crate_provenance,
                        &path,
                    )?;
                }
                let target_records = variants.entry(path.clone()).or_default();
                for (crate_id, target) in node.targets {
                    match target_records.entry(crate_id) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(target);
                        }
                        std::collections::btree_map::Entry::Occupied(entry)
                            if entry.get() != &target =>
                        {
                            return Err(CallgraphArtifactError::TargetRecordConflict {
                                def_path: path,
                                crate_id,
                                fields: entry.get().rvs_asymmetric_fields(&target),
                            });
                        }
                        std::collections::btree_map::Entry::Occupied(_) => {}
                    }
                }
            }
        }

        let mut merged = Self::rvs_new();
        let local_scope = LocalScope::rvs_new(local_crate_names);
        for (path, targets) in variants {
            let has_production_variant = targets.values().any(|target| !target.is_test_compilation);
            let non_test_sources: BTreeSet<FnSource> = targets
                .values()
                .filter(|target| !target.is_test_compilation)
                .flat_map(|target| target.sources.iter().cloned())
                .collect();
            let retained: Vec<&FnTargetData> = targets
                .values()
                .filter(|target| {
                    !target.is_test_compilation
                        || (!target.sources.is_empty()
                            && !target
                                .sources
                                .iter()
                                .any(|source| non_test_sources.contains(source)))
                        || (target.sources.is_empty() && !has_production_variant)
                })
                .collect();
            let (entries, ordinary): (Vec<_>, Vec<_>) = retained
                .into_iter()
                .partition(|target| target.is_entrypoint);

            let entry_sources: BTreeSet<FnSource> = entries
                .iter()
                .flat_map(|target| target.sources.iter().cloned())
                .collect();
            if ordinary.iter().any(|target| {
                target
                    .sources
                    .iter()
                    .any(|source| entry_sources.contains(source))
            }) {
                return Err(CallgraphArtifactError::EntrypointSourceConflict { def_path: path });
            }

            let local_ordinary = ordinary
                .iter()
                .copied()
                .filter(|target| local_scope.rvs_contains_target(&path, target.crate_provenance))
                .collect::<Vec<_>>();
            if let Some(first) = local_ordinary.first() {
                for target in local_ordinary.iter().skip(1) {
                    if first.facts.is_port_method != target.facts.is_port_method
                        || first.is_trait_impl != target.is_trait_impl
                        || first.is_test != target.is_test
                    {
                        return Err(CallgraphArtifactError::IncompatibleTargetRoles {
                            def_path: path,
                        });
                    }
                }
            }

            let mut node = FnNode {
                targets,
                ..FnNode::default()
            };
            node.rvs_rebuild_from_targets_M(&path)?;
            merged.nodes.insert(path, node);
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

fn rvs_record_crate_provenance_M(
    provenance_by_crate_id: &mut BTreeMap<u64, (CrateProvenance, DefPath)>,
    crate_id: u64,
    provenance: CrateProvenance,
    path: &DefPath,
) -> Result<(), CallgraphArtifactError> {
    if crate_id == 0 {
        return Err(CallgraphArtifactError::ZeroTargetCrateId {
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

pub(crate) fn rvs_serialize_callgraph_json_S(
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
            let targets = node.rvs_materialized_targets();
            if targets.is_empty() {
                return Err(CallgraphArtifactError::SerializeMissingTargetIdentity {
                    def_path: path.clone(),
                });
            }
            for (crate_id, target) in &targets {
                rvs_record_crate_provenance_M(
                    &mut provenance_by_crate_id,
                    *crate_id,
                    target.crate_provenance,
                    path,
                )?;
                rvs_validate_target_record(path, *crate_id, target)?;
            }
            Ok((path.clone(), FnNodeArtifact { targets }))
        })
        .collect::<Result<BTreeMap<_, _>, CallgraphArtifactError>>()?;
    let artifact = CallgraphArtifact {
        schema_version: CALLGRAPH_SCHEMA_VERSION,
        nodes,
    };
    serde_json::to_string(&artifact)
        .map_err(|source| CallgraphArtifactError::SerializeCallgraph { source })
}

pub(crate) fn rvs_serialize_function_identities_json_S(
    functions: &BTreeSet<FunctionIdentity>,
) -> Result<String, CallgraphArtifactError> {
    serde_json::to_string(functions)
        .map_err(|source| CallgraphArtifactError::SerializeFunctionIdentities { source })
}

pub(crate) fn rvs_parse_function_identities_json_S(
    json: &str,
) -> Result<BTreeSet<FunctionIdentity>, CallgraphArtifactError> {
    serde_json::from_str(json)
        .map_err(|source| CallgraphArtifactError::ParseFunctionIdentities { source })
}

/// Parse versioned or legacy callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json_S(json: &str) -> Result<FnGraph, CallgraphArtifactError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|source| CallgraphArtifactError::InvalidJson { source })?;
    let object = value
        .as_object()
        .ok_or(CallgraphArtifactError::RootMustBeObject)?;
    let is_versioned = object.contains_key("schema_version") || object.contains_key("nodes");
    let mut graph = if is_versioned {
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
        if let Some(nodes) = object.get("nodes").and_then(serde_json::Value::as_object) {
            for (def_path, node) in nodes {
                if node
                    .as_object()
                    .is_some_and(|fields| !fields.contains_key("targets"))
                {
                    return Err(CallgraphArtifactError::MissingTargets {
                        def_path: DefPath::from(def_path.as_str()),
                        schema_version: CALLGRAPH_SCHEMA_VERSION,
                    });
                }
            }
        }
        let artifact: CallgraphArtifact = serde_json::from_value(value)
            .map_err(|source| CallgraphArtifactError::InvalidVersionedRecord { source })?;
        debug_assert_eq!(artifact.schema_version, CALLGRAPH_SCHEMA_VERSION);
        let mut graph = FnGraph::rvs_new();
        for (path, node) in artifact.nodes {
            graph
                .nodes
                .insert(path.clone(), node.rvs_into_node_M(&path)?);
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
            let crate_ids = node.rvs_target_crate_ids();
            if crate_ids.contains(&0) {
                return Err(CallgraphArtifactError::ZeroTargetCrateId {
                    def_path: path.clone(),
                });
            }
            if crate_ids.is_empty() {
                return Err(CallgraphArtifactError::MissingTargetIdentity {
                    def_path: path.clone(),
                });
            }
        }
        for source in &node.sources {
            rvs_validate_fn_source(path, source)?;
        }
        for (crate_id, target) in &node.targets {
            rvs_record_crate_provenance_M(
                &mut provenance_by_crate_id,
                *crate_id,
                target.crate_provenance,
                path,
            )?;
            rvs_validate_target_record(path, *crate_id, target)?;
        }
    }
    if is_versioned {
        for (path, node) in &mut graph.nodes {
            node.rvs_rebuild_from_targets_M(path)?;
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

fn rvs_validate_target_record<'target>(
    caller: &DefPath,
    crate_id: u64,
    target: &'target FnTargetData,
) -> Result<&'target FnTargetData, CallgraphArtifactError> {
    if crate_id == 0 {
        return Err(CallgraphArtifactError::ZeroTargetCrateId {
            def_path: caller.clone(),
        });
    }
    debug_assert!(crate_id > 0, "stable crate id is nonzero");
    for source in &target.sources {
        rvs_validate_fn_source(caller, source)?;
    }
    if let Some(call) = target.calls.keys().find(|call| call.crate_id == 0) {
        return Err(CallgraphArtifactError::ZeroCalleeCrateId {
            caller: caller.clone(),
            callee: call.def_path.clone(),
        });
    }
    let mut occurrences = BTreeSet::new();
    for call_site in &target.call_sites {
        if call_site.callee.crate_id == 0 || call_site.callee.def_path.rvs_as_str().is_empty() {
            return Err(CallgraphArtifactError::InvalidCallSiteCallee {
                caller: caller.clone(),
                occurrence: call_site.occurrence,
            });
        }
        if !occurrences.insert(call_site.occurrence) {
            return Err(CallgraphArtifactError::RepeatedCallOccurrence {
                caller: caller.clone(),
                crate_id,
                occurrence: call_site.occurrence,
            });
        }
        if let Some(source) = &call_site.source {
            rvs_validate_call_site_source(source, call_site, caller)?;
        }
    }
    let site_callees = target
        .call_sites
        .iter()
        .map(|call_site| call_site.callee.clone())
        .collect::<BTreeSet<_>>();
    let call_callees = target.calls.keys().cloned().collect::<BTreeSet<_>>();
    if call_callees != site_callees {
        return Err(CallgraphArtifactError::TargetCallsMismatch {
            caller: caller.clone(),
            crate_id,
        });
    }
    Ok(target)
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
        let validated_call_site_source =
            rvs_validate_call_site_source(&call_site_source, &call_site, &caller).unwrap();
        let target = FnTargetData {
            calls: BTreeMap::from([(callee, CallEdgeType::Strong)]),
            call_sites: BTreeSet::from([call_site]),
            sources: BTreeSet::from([source.clone()]),
            ..FnTargetData::default()
        };
        let validated_target = rvs_validate_target_record(&caller, 7, &target).unwrap();

        let source_same = std::ptr::eq(validated_source, &source);
        let call_site_source_same = std::ptr::eq(validated_call_site_source, &call_site_source);
        let target_same = std::ptr::eq(validated_target, &target);
        let output = format!(
            "fn_source_same={source_same}\ncall_site_source_same={call_site_source_same}\ntarget_same={target_same}\n"
        );
        rvs_snapshot_BIS(
            "test_20260730_validators_return_original_borrowed_values",
            &output,
        );

        assert!(source_same);
        assert!(call_site_source_same);
        assert!(target_same);
    }

    #[test]
    fn test_20260730_merge_errors_preserve_borrowed_artifact_inputs() {
        let path = DefPath::from("demo::rvs_run");
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M(path.clone(), FnNode::default());
        let mut incoming = FnNode::default();
        incoming.rvs_test_capture_target_M(7, true, true);

        let node_result = target.rvs_merge_node_M(&path, &incoming);
        let node_error = matches!(
            node_result,
            Err(CallgraphArtifactError::MixedNodeFormats { .. })
        );
        let path_retained = path.rvs_as_str() == "demo::rvs_run";
        let node_retained = incoming.rvs_target_crate_ids() == BTreeSet::from([7]);

        let mut graph_target = FnGraph::rvs_new();
        graph_target.rvs_insert_M(path.clone(), FnNode::default());
        let mut graph_source = FnGraph::rvs_new();
        graph_source.rvs_insert_M(path.clone(), incoming);
        let graph_result = graph_target.rvs_merge_from_M(&graph_source);
        let graph_error = matches!(
            graph_result,
            Err(CallgraphArtifactError::MixedNodeFormats { .. })
        );
        let graph_retained = graph_source.rvs_get(path.rvs_as_str()).is_some();

        let output = format!(
            "node_error={node_error}\npath_retained={path_retained}\nnode_retained={node_retained}\ngraph_error={graph_error}\ngraph_retained={graph_retained}\n"
        );
        rvs_snapshot_BIS(
            "test_20260730_merge_errors_preserve_borrowed_artifact_inputs",
            &output,
        );

        assert!(node_error);
        assert!(path_retained);
        assert!(node_retained);
        assert!(graph_error);
        assert!(graph_retained);
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
        let result = rvs_parse_callgraph_json_S(json).unwrap();
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS("test_20260609_parse_callgraph_valid_json", &output);
        assert_eq!(result.rvs_len(), 2);
        assert!(rvs_serialize_callgraph_json_S(&result).is_err());

        let add_behavior = result
            .rvs_get("my_crate::rvs_add")
            .expect("should find rvs_add");
        assert!(add_behavior.calls.contains_key("my_crate::rvs_helper"));

        let write_behavior = result
            .rvs_get("my_crate::rvs_write_BI")
            .expect("should find rvs_write_BI");
        assert!(write_behavior.calls.contains_key("std::fs::write"));
        assert_eq!(result.rvs_values().count(), 2);
    }

    #[test]
    fn test_20260726_fn_target_view_prefers_target_facts_with_legacy_fallback() {
        let legacy = FnNode {
            facts: CapabilityFacts {
                has_async: true,
                ..CapabilityFacts::default()
            },
            has_body: false,
            is_entrypoint: true,
            ..FnNode::default()
        };
        let legacy_target = legacy.rvs_target(30);

        let mut targeted = legacy.clone();
        targeted.rvs_insert_target_M(
            10,
            FnTargetData {
                facts: CapabilityFacts {
                    is_port_method: true,
                    ..CapabilityFacts::default()
                },
                is_production: true,
                sources: BTreeSet::new(),
                ..FnTargetData::default()
            },
        );
        targeted.rvs_test_target_M(20).is_entrypoint = true;
        targeted
            .sources
            .insert(FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2));
        targeted.rvs_test_target_M(40);
        let production_target = targeted.rvs_target(10);
        let entry_target = targeted.rvs_target(20);
        let target_ids = targeted.rvs_target_crate_ids();
        let diagnostic_ids = targeted.rvs_diagnostic_crate_ids();
        let output = format!(
            "legacy: async={} port={} body={} entry={} production={} sources={}\n\
target10: async={} port={} body={} entry={} production={} sources={}\n\
target20: async={} port={} body={} entry={} production={} sources={}\n\
target_ids={target_ids:?}\n\
diagnostic_ids={diagnostic_ids:?}\n",
            legacy_target.rvs_facts().has_async,
            legacy_target.rvs_facts().is_port_method,
            legacy_target.rvs_has_body(),
            legacy_target.rvs_is_entrypoint(),
            legacy_target.rvs_is_production(),
            legacy_target.rvs_source_count(),
            production_target.rvs_facts().has_async,
            production_target.rvs_facts().is_port_method,
            production_target.rvs_has_body(),
            production_target.rvs_is_entrypoint(),
            production_target.rvs_is_production(),
            production_target.rvs_source_count(),
            entry_target.rvs_facts().has_async,
            entry_target.rvs_facts().is_port_method,
            entry_target.rvs_has_body(),
            entry_target.rvs_is_entrypoint(),
            entry_target.rvs_is_production(),
            entry_target.rvs_source_count(),
        );
        rvs_snapshot_BIS(
            "test_20260726_fn_target_view_prefers_target_facts_with_legacy_fallback",
            &output,
        );

        assert!(legacy_target.rvs_facts().has_async);
        assert!(production_target.rvs_facts().is_port_method);
        assert!(production_target.rvs_has_body());
        assert!(entry_target.rvs_is_entrypoint());
        assert_eq!(target_ids, BTreeSet::from([10, 20, 40]));
        assert_eq!(diagnostic_ids, BTreeSet::from([10, 20, 40]));
    }

    #[test]
    fn test_20260729_missing_current_target_does_not_use_aggregate_fallback() {
        let path = DefPath::from("demo::rvs_missing_target");
        let mut node = FnNode {
            facts: CapabilityFacts {
                has_async: true,
                ..CapabilityFacts::default()
            },
            is_entrypoint: true,
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            ..FnNode::default()
        };
        node.rvs_test_target_M(10);

        let missing = node.rvs_target(20);
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let classification =
            crate::function_classification::FunctionClassification::rvs_new_for_crate(
                &scope, &path, &node, 20,
            );
        let output = format!(
            "async={}\nbody={}\nentry={}\nsources={}\nreport={}\n",
            missing.rvs_facts().has_async,
            missing.rvs_has_body(),
            missing.rvs_is_entrypoint(),
            missing.rvs_source_count(),
            classification.rvs_is_report_candidate(),
        );
        rvs_snapshot_BIS(
            "test_20260729_missing_current_target_does_not_use_aggregate_fallback",
            &output,
        );

        assert!(!missing.rvs_facts().has_async);
        assert!(!missing.rvs_has_body());
        assert!(!missing.rvs_is_entrypoint());
        assert_eq!(missing.rvs_source_count(), 0);
        assert!(!classification.rvs_is_report_candidate());
    }

    #[test]
    fn test_20260726_legacy_target_records_preserve_per_target_roles() {
        let legacy = LegacyFnNodeArtifact {
            test_crate_ids: BTreeSet::from([30]),
            production_crate_ids: BTreeSet::from([10, 20]),
            entrypoint_crate_ids: BTreeSet::from([20]),
            has_body: true,
            ..LegacyFnNodeArtifact::default()
        };

        let node = legacy.rvs_into_node();
        let output = format!(
            "targets={}\ncomplete={}\nlegacy_body={}\n",
            node.targets.len(),
            node.complete,
            node.has_body,
        );
        rvs_snapshot_BIS(
            "test_20260726_legacy_target_records_preserve_per_target_roles",
            &output,
        );

        assert!(node.targets.is_empty());
        assert!(!node.complete);
        assert!(node.has_body);
    }

    #[test]
    fn test_20260726_target_record_merge_is_conservative() {
        let target = FnTargetData {
            facts: CapabilityFacts {
                has_async: true,
                ..CapabilityFacts::default()
            },
            is_test_compilation: true,
            ..FnTargetData::default()
        };
        let other = FnTargetData {
            facts: CapabilityFacts {
                has_static_ref: true,
                ..CapabilityFacts::default()
            },
            is_production: true,
            ..FnTargetData::default()
        };
        let fields = target.rvs_asymmetric_fields(&other);
        let output = format!(
            "records_equal={}\nasymmetric_fields={fields}\n",
            target == other,
        );
        rvs_snapshot_BIS("test_20260726_target_record_merge_is_conservative", &output);

        assert_ne!(target, other);
        assert!(fields.contains("facts"));
        assert!(fields.contains("is_test_compilation"));
        assert!(fields.contains("is_production"));
    }

    #[test]
    fn test_20260729_artifact_merge_rejects_conflicting_crate_provenance() {
        let make_graph = |path: &str, provenance| {
            let mut node = FnNode::default();
            node.rvs_test_target_M(7).crate_provenance = provenance;
            let mut graph = FnGraph::rvs_new();
            graph.rvs_insert_M(DefPath::from(path), node);
            graph
        };
        let result = FnGraph::rvs_merge_artifacts(
            vec![
                make_graph(
                    "build_script_build::rvs_local",
                    CrateProvenance::PrimaryPackage,
                ),
                make_graph(
                    "build_script_build::rvs_dependency",
                    CrateProvenance::Dependency,
                ),
            ],
            &BTreeSet::from([CrateName::from("build_script_build")]),
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
        let mut node = FnNode::default();
        node.rvs_insert_target_M(
            7,
            FnTargetData {
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
                ..FnTargetData::default()
            },
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run_A"), node);
        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let base: serde_json::Value = serde_json::from_str(&json).unwrap();

        let mut cases = Vec::new();
        let mut top_level = base.clone();
        top_level["future_top_level"] = true.into();
        cases.push(("top_level", top_level));

        let mut node = base.clone();
        node["nodes"]["demo::rvs_run_A"]["future_node"] = true.into();
        cases.push(("node", node));

        let mut target = base.clone();
        target["nodes"]["demo::rvs_run_A"]["targets"]["7"]["future_target"] = true.into();
        cases.push(("target", target));

        let mut function_identity = base.clone();
        function_identity["nodes"]["demo::rvs_run_A"]["targets"]["7"]["calls"][0]["future_identity"] =
            true.into();
        cases.push(("function_identity", function_identity));

        let mut call_site = base.clone();
        call_site["nodes"]["demo::rvs_run_A"]["targets"]["7"]["call_sites"][0]["future_call_site"] =
            true.into();
        cases.push(("call_site", call_site));

        let mut call_site_source = base.clone();
        call_site_source["nodes"]["demo::rvs_run_A"]["targets"]["7"]["call_sites"][0]["source"]["future_call_site_source"] =
            true.into();
        cases.push(("call_site_source", call_site_source));

        let mut fact = base.clone();
        fact["nodes"]["demo::rvs_run_A"]["targets"]["7"]["facts"]["future_fact"] = true.into();
        cases.push(("fact", fact));

        let mut source = base.clone();
        source["nodes"]["demo::rvs_run_A"]["targets"]["7"]["sources"][0]["future_source"] =
            true.into();
        cases.push(("source", source));

        let mut provenance = base;
        provenance["nodes"]["demo::rvs_run_A"]["targets"]["7"]["crate_provenance"] =
            serde_json::from_str(r#"{"kind":"primary_package","future_provenance":true}"#).unwrap();
        cases.push(("provenance", provenance));

        let mut output = String::new();
        for (name, value) in cases {
            let accepted =
                rvs_parse_callgraph_json_S(&serde_json::to_string(&value).unwrap()).is_ok();
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
        let mut graph = rvs_parse_callgraph_json_S(json).unwrap();
        let node = graph.rvs_get("demo::rvs_legacy_A").unwrap();
        let target_count = node.targets.len();
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let report_candidate = crate::function_classification::FunctionClassification::rvs_new(
            &scope,
            &DefPath::from("demo::rvs_legacy_A"),
            node,
        )
        .rvs_is_report_candidate();
        let serialize_error = rvs_serialize_callgraph_json_S(&graph).is_err();
        let analysis = crate::inference::PreparedInference::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let inference_incomplete = analysis
            .rvs_incomplete_paths()
            .contains(&DefPath::from("demo::rvs_legacy_A"));
        let empty_legacy = rvs_parse_callgraph_json_S("{}").unwrap();
        let empty_serialize_error = rvs_serialize_callgraph_json_S(&empty_legacy).is_err();
        let output = format!(
            "target_count={target_count}\nserialize_error={serialize_error}\nempty_serialize_error={empty_serialize_error}\nreport_candidate={report_candidate}\ninference_incomplete={inference_incomplete}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_legacy_callgraph_remains_uncertain_and_read_only",
            &output,
        );

        assert_eq!(target_count, 0);
        assert!(serialize_error);
        assert!(empty_serialize_error);
        assert!(!report_candidate);
        assert!(inference_incomplete);
    }

    #[test]
    fn test_20260729_artifact_merge_rejects_legacy_current_mixture() {
        let legacy =
            rvs_parse_callgraph_json_S(r#"{"legacy::rvs_read":{"calls":[],"has_body":true}}"#)
                .unwrap();
        let mut current_node = FnNode::default();
        current_node.rvs_test_target_M(7).crate_provenance = CrateProvenance::PrimaryPackage;
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
    fn test_20260729_target_record_merge_rejects_asymmetric_metadata() {
        let rvs_graph = |has_async| {
            let mut node = FnNode::default();
            node.rvs_insert_target_M(
                7,
                FnTargetData {
                    facts: CapabilityFacts {
                        has_async,
                        ..CapabilityFacts::default()
                    },
                    crate_provenance: CrateProvenance::PrimaryPackage,
                    ..FnTargetData::default()
                },
            );
            let mut graph = FnGraph::rvs_new();
            graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
            graph
        };
        let result = std::panic::catch_unwind(|| {
            FnGraph::rvs_merge_artifacts(
                vec![rvs_graph(false), rvs_graph(true)],
                &BTreeSet::from([CrateName::from("demo")]),
            )
        });
        let panicked = result.is_err();
        let merge_error = result.is_ok_and(|result| result.is_err());
        let output = format!("panicked={panicked}\nmerge_error={merge_error}\n");
        rvs_snapshot_BIS(
            "test_20260729_target_record_merge_rejects_asymmetric_metadata",
            &output,
        );

        assert!(!panicked);
        assert!(merge_error);
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
        let mut node = FnNode::default();
        node.rvs_insert_target_M(
            7,
            FnTargetData {
                calls: BTreeMap::from([(callee, CallEdgeType::Strong)]),
                call_sites,
                crate_provenance: CrateProvenance::PrimaryPackage,
                ..FnTargetData::default()
            },
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json_S(&json).unwrap();
        let call_site_count = parsed
            .rvs_get("demo::rvs_run")
            .and_then(|node| node.targets.get(&7))
            .map_or(0, |target| target.call_sites.len());

        let mut repeated_occurrence: serde_json::Value = serde_json::from_str(&json).unwrap();
        repeated_occurrence["nodes"]["demo::rvs_run"]["targets"]["7"]["call_sites"][1]["occurrence"] =
            0.into();
        let repeated_occurrence_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&repeated_occurrence).unwrap())
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
            .and_then(|merged| rvs_serialize_callgraph_json_S(merged).ok());
        let reverse = reverse_graph
            .as_ref()
            .ok()
            .and_then(|merged| rvs_serialize_callgraph_json_S(merged).ok());
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
        let mut node = FnNode::default();
        node.rvs_insert_target_M(
            7,
            FnTargetData {
                calls,
                call_sites,
                crate_provenance: CrateProvenance::PrimaryPackage,
                ..FnTargetData::default()
            },
        );
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_order"), node);
        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json_S(&json).unwrap();
        let target = parsed
            .rvs_get("demo::rvs_order")
            .and_then(|node| node.targets.get(&7))
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
            rvs_parse_callgraph_json_S(r#"{"schema_version":12,"nodes":{"demo::rvs_run""#)
        });
        let incompatible = std::panic::catch_unwind(|| {
            rvs_parse_callgraph_json_S(r#"{"schema_version":11,"nodes":{}}"#)
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
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.rvs_test_capture_target_M(7, true, true);
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
        missing_facts["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("facts");
        let missing_facts_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_facts).unwrap())
                .unwrap_err();
        let mut missing_has_body: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_has_body["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("has_body");
        let missing_has_body_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_has_body).unwrap())
                .unwrap_err();
        let mut missing_call_sites: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_call_sites["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("call_sites");
        let missing_call_sites_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_call_sites).unwrap())
                .unwrap_err();
        let mut missing_entrypoints: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_entrypoints["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("is_entrypoint");
        let missing_entrypoints_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_entrypoints).unwrap())
                .unwrap_err();
        let mut missing_provenance: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_provenance["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("crate_provenance");
        let missing_provenance_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_provenance).unwrap())
                .unwrap_err();
        let version_marker = format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#);
        let output = format!(
            "schema_version={CALLGRAPH_SCHEMA_VERSION}\ncontains_version={}\nnodes={}\nprevious_version_error={previous_version_error}\nmissing_facts_error={missing_facts_error}\nmissing_has_body_error={missing_has_body_error}\nmissing_call_sites_error={missing_call_sites_error}\nmissing_entrypoints_error={missing_entrypoints_error}\nmissing_provenance_error={missing_provenance_error}\n",
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
    fn test_20260730_schema_12_requires_every_capability_fact() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.rvs_test_capture_target_M(7, true, true);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let base: serde_json::Value = serde_json::from_str(&json).unwrap();

        let fact_fields = [
            "has_async",
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
            missing["nodes"]["demo::rvs_run"]["targets"]["7"]["facts"]
                .as_object_mut()
                .expect("never: serialized capability facts are an object")
                .remove(field);
            let is_rejected =
                rvs_parse_callgraph_json_S(&serde_json::to_string(&missing).unwrap()).is_err();
            rejected.push(is_rejected);
            output.push_str(&format!("{field}: rejected={is_rejected}\n"));
        }

        let legacy = rvs_parse_callgraph_json_S(
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
            "test_20260730_schema_12_requires_every_capability_fact",
            &output,
        );

        assert!(rejected.into_iter().all(|is_rejected| is_rejected));
        assert!(legacy.is_ok());
        assert!(legacy_async);
        assert!(!legacy_static_mut);
    }

    #[test]
    fn test_20260716_callgraph_artifact_rejects_identityless_node() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), FnNode::default());

        let error = rvs_serialize_callgraph_json_S(&graph).unwrap_err();
        let output = format!("{error}\n");
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_rejects_identityless_node",
            &output,
        );

        assert!(error.to_string().contains("target identity"));
    }

    #[test]
    fn test_20260715_callgraph_artifact_requires_facts_for_each_crate_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.rvs_test_capture_target_M(7, true, false);
        let path = DefPath::from("demo::rvs_run");
        graph.rvs_insert_M(path.clone(), node);

        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let mut missing_facts: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_facts["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("facts");
        let missing_facts_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_facts).unwrap())
                .unwrap_err();
        let mut missing_has_body: serde_json::Value = serde_json::from_str(&json).unwrap();
        missing_has_body["nodes"]["demo::rvs_run"]["targets"]["7"]
            .as_object_mut()
            .expect("never: serialized target is an object")
            .remove("has_body");
        let missing_has_body_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_has_body).unwrap())
                .unwrap_err();
        let output = format!(
            "missing_facts={missing_facts_error}\nmissing_has_body={missing_has_body_error}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_callgraph_artifact_requires_facts_for_each_crate_identity",
            &output,
        );

        assert!(missing_facts_error.to_string().contains("facts"));
        assert!(missing_has_body_error.to_string().contains("has_body"));
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

        assert!(output.contains("UnsupportedSchemaVersion"));
        assert!(output.contains("InvalidSchemaVersion"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_validates_target_entrypoints() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode {
            is_entrypoint: true,
            ..FnNode::default()
        };
        node.rvs_test_capture_target_M(7, true, false);
        graph.rvs_insert_M(DefPath::from("demo::main"), node);

        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let valid = rvs_parse_callgraph_json_S(&valid_json);
        let mut ordinary: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        ordinary["nodes"]["demo::main"]["targets"]["7"]["is_entrypoint"] = false.into();
        let ordinary =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&ordinary).unwrap()).unwrap();
        let output = format!(
            "valid={}\nentrypoint={}\nordinary={}\n",
            valid.is_ok(),
            valid
                .as_ref()
                .ok()
                .and_then(|graph| graph.rvs_get("demo::main"))
                .is_some_and(|node| node.is_entrypoint),
            ordinary
                .rvs_get("demo::main")
                .is_some_and(|node| !node.is_entrypoint),
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_validates_target_entrypoints",
            &output,
        );

        assert!(valid.is_ok());
        assert!(
            ordinary
                .rvs_get("demo::main")
                .is_some_and(|node| !node.is_entrypoint)
        );
    }

    #[test]
    fn test_20260716_callgraph_artifact_rejects_asymmetric_target_metadata() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.rvs_test_capture_target_M(7, true, false);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);

        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let mut parallel_metadata: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        parallel_metadata["nodes"]["demo::rvs_run"]["facts_by_crate"] = serde_json::to_value(
            BTreeMap::from([("8".to_string(), CapabilityFacts::default())]),
        )
        .unwrap();
        let parallel_metadata_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&parallel_metadata).unwrap())
                .unwrap_err();

        let mut zero_id: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        let zero_target = zero_id["nodes"]["demo::rvs_run"]["targets"]["7"].clone();
        zero_id["nodes"]["demo::rvs_run"]["targets"]["0"] = zero_target;
        let zero_id_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&zero_id).unwrap()).unwrap_err();

        let mut invalid_source: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        invalid_source["nodes"]["demo::rvs_run"]["targets"]["7"]["sources"] =
            serde_json::from_str(r#"[{"file":"","name_start":1,"name_end":2}]"#).unwrap();
        let invalid_source_error =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&invalid_source).unwrap())
                .unwrap_err();

        let output = format!(
            "parallel_metadata={parallel_metadata_error}\nzero_id={zero_id_error}\ninvalid_source={invalid_source_error}\n"
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_rejects_asymmetric_target_metadata",
            &output,
        );

        assert!(
            parallel_metadata_error
                .to_string()
                .contains("unknown field")
        );
        assert!(zero_id_error.to_string().contains("zero crate id"));
        assert!(invalid_source_error.to_string().contains("source file"));
    }

    #[test]
    fn test_20260716_callgraph_artifact_validates_call_site_coverage_consistency() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        let first = FunctionIdentity {
            crate_id: 9,
            def_path: DefPath::from("dependency::first"),
        };
        let second = FunctionIdentity {
            crate_id: 10,
            def_path: DefPath::from("dependency::second"),
        };
        node.rvs_insert_target_M(
            7,
            FnTargetData {
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
                ..FnTargetData::default()
            },
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        let valid_json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let mut missing_site_json: serde_json::Value = serde_json::from_str(&valid_json).unwrap();
        missing_site_json["nodes"]["demo::rvs_run"]["targets"]["7"]["call_sites"]
            .as_array_mut()
            .expect("never: serialized call sites are an array")
            .pop();
        let missing_site =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&missing_site_json).unwrap())
                .unwrap_err();
        let mut duplicate_occurrence_json: serde_json::Value =
            serde_json::from_str(&valid_json).unwrap();
        duplicate_occurrence_json["nodes"]["demo::rvs_run"]["targets"]["7"]["call_sites"][1]["occurrence"] =
            0.into();
        let duplicate_occurrence =
            rvs_parse_callgraph_json_S(&serde_json::to_string(&duplicate_occurrence_json).unwrap())
                .unwrap_err();
        let valid = rvs_parse_callgraph_json_S(&valid_json);
        let output = format!(
            "missing_site={missing_site}\nduplicate_occurrence={duplicate_occurrence}\nvalid={}\n",
            valid.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_callgraph_artifact_validates_call_site_coverage_consistency",
            &output,
        );

        assert!(missing_site.to_string().contains("target call sites"));
        assert!(duplicate_occurrence.to_string().contains("occurrence"));
        assert!(valid.is_ok());
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
        base.rvs_merge_from_M(&extended).unwrap();

        let mut merged = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.calls.insert(
            DefPath::from("std::fs::read_to_string"),
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
    fn test_20260713_artifact_merge_resolves_entrypoint_roles() {
        let local_crate_names = BTreeSet::from([CrateName::from("demo")]);
        let ordinary_source = FnSource::rvs_new(PathBuf::from("src/lib.rs"), 7, 11);
        let entry_source = FnSource::rvs_new(PathBuf::from("src/main.rs"), 3, 7);
        let mut ordinary = FnNode {
            sources: BTreeSet::from([ordinary_source.clone()]),
            ..FnNode::default()
        };
        ordinary.calls.insert(
            DefPath::from("std::fs::read_to_string"),
            CallEdgeType::Strong,
        );
        ordinary.rvs_test_capture_target_M(1, true, true);
        let mut entry = FnNode {
            is_entrypoint: true,
            sources: BTreeSet::from([entry_source]),
            ..FnNode::default()
        };
        entry
            .calls
            .insert(DefPath::from("std::process::exit"), CallEdgeType::Strong);
        entry.rvs_test_capture_target_M(2, true, false);

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
        test_copy.calls.insert(
            DefPath::from("test::test_main_static"),
            CallEdgeType::Strong,
        );
        test_copy.rvs_test_capture_target_M(3, false, false);
        test_copy.rvs_test_target_M(3).is_test = true;
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
            .insert(DefPath::from("std::process::exit"), CallEdgeType::Strong);
        shared_entry.rvs_test_capture_target_M(4, true, false);
        let mut conflict = FnGraph::rvs_new();
        conflict.rvs_insert_M(DefPath::from("demo::main"), ordinary);
        let mut conflicting_artifact = FnGraph::rvs_new();
        conflicting_artifact.rvs_insert_M(DefPath::from("demo::main"), shared_entry);
        let conflict_result =
            FnGraph::rvs_merge_artifacts(vec![conflict, conflicting_artifact], &local_crate_names);

        let mut first_ordinary = FnGraph::rvs_new();
        let mut first_ordinary_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 20, 27)]),
            report_line_count: Some(2),
            report_function_count: 1,
            ..FnNode::default()
        };
        first_ordinary_node.rvs_test_capture_target_M(10, true, true);
        first_ordinary.rvs_insert_M(DefPath::from("demo::rvs_run"), first_ordinary_node);
        let mut conflicting_ordinary_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/main.rs"), 20, 27)]),
            report_line_count: Some(3),
            report_function_count: 1,
            ..FnNode::default()
        };
        conflicting_ordinary_node
            .calls
            .insert(DefPath::from("dep::effect_S"), CallEdgeType::Strong);
        conflicting_ordinary_node.rvs_test_capture_target_M(20, true, true);
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
        let mut ordinary_variant_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/lib.rs"), 30, 39)]),
            ..FnNode::default()
        };
        ordinary_variant_node.rvs_test_capture_target_M(30, true, true);
        ordinary_variant.rvs_insert_M(DefPath::from("demo::rvs_fetch"), ordinary_variant_node);
        let mut port_variant_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/main.rs"), 30, 39)]),
            ..FnNode::default()
        };
        port_variant_node.facts.is_port_method = true;
        port_variant_node.rvs_test_capture_target_M(40, true, true);
        let mut port_variant = FnGraph::rvs_new();
        port_variant.rvs_insert_M(DefPath::from("demo::rvs_fetch"), port_variant_node);
        let mixed_role =
            FnGraph::rvs_merge_artifacts(vec![ordinary_variant, port_variant], &local_crate_names);

        let mut source_less_ordinary = FnGraph::rvs_new();
        let mut source_less_ordinary_node = FnNode::default();
        source_less_ordinary_node.rvs_test_capture_target_M(50, true, true);
        source_less_ordinary.rvs_insert_M(
            DefPath::from("demo::rvs_source_less"),
            source_less_ordinary_node,
        );
        let mut source_less_port_node = FnNode::default();
        source_less_port_node.facts.is_port_method = true;
        source_less_port_node.rvs_test_capture_target_M(60, true, true);
        let mut source_less_port = FnGraph::rvs_new();
        source_less_port.rvs_insert_M(
            DefPath::from("demo::rvs_source_less"),
            source_less_port_node,
        );
        let source_less_mixed_role = FnGraph::rvs_merge_artifacts(
            vec![source_less_ordinary, source_less_port],
            &local_crate_names,
        );
        let retained_entrypoint_ids: BTreeSet<u64> = retained
            .targets
            .iter()
            .filter_map(|(crate_id, target)| target.is_entrypoint.then_some(*crate_id))
            .collect();
        let reverse_entrypoint_ids: BTreeSet<u64> = reverse_retained
            .targets
            .iter()
            .filter_map(|(crate_id, target)| target.is_entrypoint.then_some(*crate_id))
            .collect();

        let output = format!(
            "retained_entry={}\nentrypoint_crate_ids={:?}\nretained_sources={:?}\nretained_calls={:?}\nentry_calls={:?}\nreverse_equal={}\nentry_conflict={conflict_result:?}\nordinary_calls={:?}\nordinary_function_count={}\nordinary_line_count={:?}\nmixed_role={mixed_role:?}\nsource_less_mixed_role={source_less_mixed_role:?}\n",
            retained.is_entrypoint,
            retained_entrypoint_ids,
            retained.sources,
            retained.calls,
            retained.entry_calls,
            retained.sources == reverse_retained.sources
                && retained.calls == reverse_retained.calls
                && retained.entry_calls == reverse_retained.entry_calls
                && retained.is_entrypoint == reverse_retained.is_entrypoint
                && retained_entrypoint_ids == reverse_entrypoint_ids,
            ordinary_merged.calls,
            ordinary_merged.report_function_count,
            ordinary_merged.report_line_count,
        );
        rvs_snapshot_BIS(
            "test_20260713_artifact_merge_resolves_entrypoint_roles",
            &output,
        );

        assert!(!retained.is_entrypoint);
        assert_eq!(retained_entrypoint_ids, BTreeSet::from([2]));
        assert_eq!(retained.sources.len(), 1);
        assert!(retained.calls.contains_key("std::fs::read_to_string"));
        assert!(!retained.calls.contains_key("std::process::exit"));
        assert!(retained.entry_calls.contains_key("std::process::exit"));
        assert!(!retained.entry_calls.contains_key("test::test_main_static"));
        assert_eq!(retained.sources, reverse_retained.sources);
        assert_eq!(retained.calls, reverse_retained.calls);
        assert_eq!(retained.entry_calls, reverse_retained.entry_calls);
        assert!(conflict_result.is_err());
        assert!(ordinary_merge.is_ok());
        assert!(ordinary_merged.calls.contains_key("dep::effect_S"));
        assert_eq!(ordinary_merged.report_function_count, 2);
        assert_eq!(ordinary_merged.report_line_count, Some(5));
        assert!(mixed_role.is_err());
        assert!(source_less_mixed_role.is_err());
    }

    #[test]
    fn test_20260716_artifact_merge_keeps_entrypoint_calls_separate() {
        let path = DefPath::from("demo::main");
        let ordinary_callee = FunctionIdentity {
            crate_id: 100,
            def_path: DefPath::from("dependency::ordinary"),
        };
        let entry_callee = FunctionIdentity {
            crate_id: 200,
            def_path: DefPath::from("dependency::shutdown_S"),
        };

        let mut ordinary = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 7, 11)]),
            ..FnNode::default()
        };
        ordinary
            .calls
            .insert(ordinary_callee.def_path.clone(), CallEdgeType::Strong);
        ordinary.rvs_insert_target_M(
            10,
            FnTargetData {
                calls: BTreeMap::from([(ordinary_callee.clone(), CallEdgeType::Strong)]),
                call_sites: BTreeSet::from([CallSiteIdentity {
                    callee: ordinary_callee,
                    occurrence: 0,
                    source: None,
                }]),
                sources: ordinary.sources.clone(),
                is_production: true,
                is_coverage_candidate: true,
                ..FnTargetData::default()
            },
        );

        let mut entry = FnNode {
            is_entrypoint: true,
            sources: BTreeSet::from([FnSource::rvs_new("src/main.rs".into(), 3, 7)]),
            ..FnNode::default()
        };
        entry
            .calls
            .insert(entry_callee.def_path.clone(), CallEdgeType::Strong);
        entry.rvs_insert_target_M(
            20,
            FnTargetData {
                calls: BTreeMap::from([(entry_callee.clone(), CallEdgeType::Strong)]),
                call_sites: BTreeSet::from([CallSiteIdentity {
                    callee: entry_callee,
                    occurrence: 0,
                    source: None,
                }]),
                is_entrypoint: true,
                sources: entry.sources.clone(),
                is_production: true,
                ..FnTargetData::default()
            },
        );

        let mut ordinary_graph = FnGraph::rvs_new();
        ordinary_graph.rvs_insert_M(path.clone(), ordinary);
        let mut entry_graph = FnGraph::rvs_new();
        entry_graph.rvs_insert_M(path.clone(), entry);
        let merged = FnGraph::rvs_merge_artifacts(
            vec![ordinary_graph, entry_graph],
            &BTreeSet::from([CrateName::from("demo")]),
        )
        .unwrap();
        let json = rvs_serialize_callgraph_json_S(&merged).unwrap();
        let parsed = rvs_parse_callgraph_json_S(&json);
        let node = merged.rvs_get(path.rvs_as_str()).unwrap();
        let output = format!(
            "calls={:?}\nentry_calls={:?}\nparse_ok={}\n",
            node.calls,
            node.entry_calls,
            parsed.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_artifact_merge_keeps_entrypoint_calls_separate",
            &output,
        );

        assert_eq!(
            node.calls,
            BTreeMap::from([(DefPath::from("dependency::ordinary"), CallEdgeType::Strong)])
        );
        assert_eq!(
            node.entry_calls,
            BTreeMap::from([(
                DefPath::from("dependency::shutdown_S"),
                CallEdgeType::Strong
            )])
        );
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_20260716_artifact_merge_same_source_line_count_is_deterministic() {
        let path = DefPath::from("demo::rvs_run");
        let source = FnSource::rvs_new("src/lib.rs".into(), 7, 14);
        let make_graph = |line_count| {
            let mut graph = FnGraph::rvs_new();
            let mut node = FnNode {
                sources: BTreeSet::from([source.clone()]),
                report_line_count: Some(line_count),
                report_function_count: 1,
                ..FnNode::default()
            };
            node.rvs_test_capture_target_M(line_count as u64, true, true);
            graph.rvs_insert_M(path.clone(), node);
            graph
        };
        let local = BTreeSet::from([CrateName::from("demo")]);
        let forward =
            FnGraph::rvs_merge_artifacts(vec![make_graph(2), make_graph(3)], &local).unwrap();
        let reverse =
            FnGraph::rvs_merge_artifacts(vec![make_graph(3), make_graph(2)], &local).unwrap();
        let forward_count = forward
            .rvs_get(path.rvs_as_str())
            .and_then(|node| node.report_line_count);
        let reverse_count = reverse
            .rvs_get(path.rvs_as_str())
            .and_then(|node| node.report_line_count);
        let output = format!("forward={forward_count:?}\nreverse={reverse_count:?}\n");
        rvs_snapshot_BIS(
            "test_20260716_artifact_merge_same_source_line_count_is_deterministic",
            &output,
        );

        assert_eq!(forward_count, Some(3));
        assert_eq!(reverse_count, Some(3));
    }

    #[test]
    fn test_20260715_artifact_merge_treats_local_trait_external_type_impl_as_local() {
        let path = DefPath::from(
            "std::fs::File{impl#7374643a3a66733a3a46696c654064656d6f3a3a46696c65436c69656e74}::rvs_touch_P@demo::FileClient",
        );
        let mut first = FnGraph::rvs_new();
        let mut first_node = FnNode {
            is_trait_impl: true,
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            report_line_count: Some(2),
            report_function_count: 1,
            ..FnNode::default()
        };
        first_node.rvs_test_capture_target_M(1, true, true);
        first.rvs_insert_M(path.clone(), first_node);
        let mut second = FnGraph::rvs_new();
        let mut second_node = FnNode {
            is_trait_impl: true,
            sources: BTreeSet::from([FnSource::rvs_new("src/other.rs".into(), 1, 2)]),
            report_line_count: Some(3),
            report_function_count: 1,
            ..FnNode::default()
        };
        second_node.rvs_test_capture_target_M(2, true, true);
        second.rvs_insert_M(path.clone(), second_node);

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
        production.rvs_test_capture_target_M(1, true, true);
        let mut test_copy = FnNode {
            is_test_compilation: true,
            ..FnNode::default()
        };
        test_copy.calls.insert(
            DefPath::from("demo::rvs_test_only_dependency"),
            CallEdgeType::Strong,
        );
        test_copy.rvs_test_capture_target_M(2, false, false);
        let mut production_graph = FnGraph::rvs_new();
        production_graph.rvs_insert_M(DefPath::from("demo::rvs_generated"), production);
        let mut test_graph = FnGraph::rvs_new();
        test_graph.rvs_insert_M(DefPath::from("demo::rvs_generated"), test_copy);

        let merged =
            FnGraph::rvs_merge_artifacts(vec![production_graph, test_graph], &local_crate_names)
                .unwrap();
        let node = merged.rvs_get("demo::rvs_generated").unwrap();
        let production_ids: BTreeSet<u64> = node
            .targets
            .iter()
            .filter_map(|(crate_id, target)| target.is_production.then_some(*crate_id))
            .collect();
        let output = format!(
            "is_test_compilation={}\nsources={}\nproduction_ids={:?}\n",
            node.is_test_compilation,
            node.sources.len(),
            production_ids,
        );
        rvs_snapshot_BIS(
            "test_20260714_source_less_production_node_survives_test_merge",
            &output,
        );

        assert!(!node.is_test_compilation);
        assert!(node.sources.is_empty());
        assert_eq!(production_ids, BTreeSet::from([1]));
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
        with_body
            .calls
            .insert(DefPath::from("dep::rvs_call_BI"), CallEdgeType::Strong);
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
        let mut test_node = FnNode::default();
        test_node.rvs_test_target_M(1).is_test = true;
        test_node.rvs_test_target_M(1).calls = BTreeMap::from([
            (strong_target.clone(), CallEdgeType::Strong),
            (weak_target.clone(), CallEdgeType::Weak),
        ]);
        let mut strong_node = FnNode::default();
        strong_node.rvs_test_target_M(1).calls = BTreeMap::from([
            (strong_leaf.clone(), CallEdgeType::Strong),
            (weak_leaf.clone(), CallEdgeType::Weak),
        ]);
        let mut weak_node = FnNode::default();
        weak_node.rvs_test_target_M(1).calls =
            BTreeMap::from([(hidden_behind_weak_root.clone(), CallEdgeType::Strong)]);
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
