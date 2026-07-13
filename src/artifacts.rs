use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityFacts;
use crate::symbols::{CrateName, DefPath};

pub(crate) const CALLGRAPH_SCHEMA_VERSION: u32 = 2;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FnNode {
    pub calls: BTreeSet<DefPath>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub entry_calls: BTreeSet<DefPath>,
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
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub allows_dead_code: bool,
}

impl Default for FnNode {
    fn default() -> Self {
        Self {
            calls: BTreeSet::new(),
            entry_calls: BTreeSet::new(),
            facts: CapabilityFacts::default(),
            has_body: true,
            is_trait_impl: false,
            is_test: false,
            is_entrypoint: false,
            is_test_compilation: false,
            sources: BTreeSet::new(),
            report_line_count: None,
            allows_dead_code: false,
        }
    }
}

impl FnNode {
    /// Merge another callgraph entry for the same function into this one.
    pub fn rvs_merge_M(&mut self, other: &Self) {
        self.calls.extend(other.calls.iter().cloned());
        self.entry_calls.extend(other.entry_calls.iter().cloned());
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
        self.allows_dead_code |= other.allows_dead_code;
    }

    pub(crate) fn rvs_dependency_calls(&self) -> impl Iterator<Item = &DefPath> {
        self.calls.iter().chain(&self.entry_calls)
    }

    fn rvs_has_same_merge_behavior(&self, other: &Self) -> bool {
        self.calls == other.calls
            && self.entry_calls == other.entry_calls
            && self.facts == other.facts
            && self.has_body == other.has_body
            && self.is_trait_impl == other.is_trait_impl
            && self.is_test == other.is_test
            && self.is_entrypoint == other.is_entrypoint
            && self.report_line_count == other.report_line_count
            && self.allows_dead_code == other.allows_dead_code
    }
}

fn rvs_is_false(value: &bool) -> bool {
    !*value
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
        for (path, nodes) in variants {
            let is_local = local_crate_names.iter().any(|crate_name| {
                path.rvs_as_str()
                    .starts_with(crate_name.rvs_prefix().rvs_as_str())
            });
            let has_production_entry = nodes.iter().any(|node| node.is_entrypoint);
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
                        || (node.sources.is_empty() && !has_production_entry)
                })
                .collect();
            let (mut entries, mut ordinary): (Vec<_>, Vec<_>) =
                retained.into_iter().partition(|node| node.is_entrypoint);

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
            } else {
                for node in ordinary {
                    if is_local && !selected.rvs_has_same_merge_behavior(&node) {
                        return Err(format!(
                            "function {path} has conflicting ordinary definitions across Cargo targets"
                        ));
                    }
                    selected.rvs_merge_M(&node);
                }
                for entry in entries {
                    selected
                        .entry_calls
                        .extend(entry.rvs_dependency_calls().cloned());
                }
            }
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

/// Parse versioned or legacy callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json_S(json: &str) -> Result<FnGraph, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "invalid callgraph JSON: root must be an object".to_string())?;
    let graph = if object.contains_key("schema_version") || object.contains_key("nodes") {
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
        for source in &node.sources {
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
    }
    Ok(graph)
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
        let output = format!("{result:?}");
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
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), FnNode::default());

        let json = rvs_serialize_callgraph_json_S(&graph).unwrap();
        let parsed = rvs_parse_callgraph_json_S(&json).unwrap();
        let version_marker = format!(r#""schema_version":{CALLGRAPH_SCHEMA_VERSION}"#);
        let output = format!(
            "schema_version={CALLGRAPH_SCHEMA_VERSION}\ncontains_version={}\nnodes={}\n",
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
    fn test_20260710_callgraph_artifact_schema_validation() {
        let cases = [
            ("unknown", r#"{"schema_version":3,"nodes":{}}"#),
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

        assert!(output.contains("unsupported callgraph schema version 3"));
        assert!(output.contains("schema_version must be an unsigned integer"));
    }

    #[test]
    fn test_20260703_graph_extend_and_merge_helpers() {
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
        let mut entry = FnNode {
            is_entrypoint: true,
            sources: BTreeSet::from([entry_source]),
            ..FnNode::default()
        };
        entry.calls.insert(DefPath::from("std::process::exit"));

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
                ..FnNode::default()
            },
        );
        let mut conflicting_ordinary_node = FnNode {
            sources: BTreeSet::from([FnSource::rvs_new(PathBuf::from("src/main.rs"), 20, 27)]),
            ..FnNode::default()
        };
        conflicting_ordinary_node
            .calls
            .insert(DefPath::from("dep::effect_S"));
        let mut second_ordinary = FnGraph::rvs_new();
        second_ordinary.rvs_insert_M(DefPath::from("demo::rvs_run"), conflicting_ordinary_node);
        let ordinary_conflict =
            FnGraph::rvs_merge_artifacts(vec![first_ordinary, second_ordinary], &local_crate_names);

        let output = format!(
            "retained_entry={}\nretained_sources={:?}\nretained_calls={:?}\nentry_calls={:?}\nreverse_equal={}\nentry_conflict={conflict_result:?}\nordinary_conflict={ordinary_conflict:?}\n",
            retained.is_entrypoint,
            retained.sources,
            retained.calls,
            retained.entry_calls,
            retained.sources == reverse_retained.sources
                && retained.calls == reverse_retained.calls
                && retained.entry_calls == reverse_retained.entry_calls
                && retained.is_entrypoint == reverse_retained.is_entrypoint,
        );
        rvs_snapshot_BIS(
            "test_20260713_artifact_merge_resolves_entrypoint_roles",
            &output,
        );

        assert!(!retained.is_entrypoint);
        assert_eq!(retained.sources.len(), 1);
        assert!(retained.calls.contains("std::fs::read_to_string"));
        assert!(!retained.calls.contains("std::process::exit"));
        assert!(retained.entry_calls.contains("std::process::exit"));
        assert!(!retained.entry_calls.contains("test::test_main_static"));
        assert_eq!(retained.sources, reverse_retained.sources);
        assert_eq!(retained.calls, reverse_retained.calls);
        assert_eq!(retained.entry_calls, reverse_retained.entry_calls);
        assert!(conflict_result.is_err());
        assert!(ordinary_conflict.is_err());
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
