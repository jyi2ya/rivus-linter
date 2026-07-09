use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::capability::{CapabilityFacts, CapabilitySet};
use crate::symbols::{DefPath, FnName};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnSource {
    pub file: PathBuf,
    pub name_start: u32,
    pub name_end: u32,
}

impl FnSource {
    pub(crate) fn rvs_new(file: PathBuf, name_start: u32, name_end: u32) -> Self {
        debug_assert!(name_start < name_end, "source name range must be non-empty");
        Self {
            file,
            name_start,
            name_end,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FnNode {
    pub calls: BTreeSet<DefPath>,
    #[serde(flatten)]
    pub facts: CapabilityFacts,
    pub has_body: bool,
    #[serde(default)]
    pub is_trait_impl: bool,
    #[serde(default)]
    pub is_test: bool,
    #[serde(default)]
    pub sources: BTreeSet<FnSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_caps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_line_count: Option<usize>,
    #[serde(default, skip_serializing_if = "rvs_is_false")]
    pub allows_dead_code: bool,
    #[serde(skip)]
    pub is_synthetic: bool,
    #[serde(skip)]
    pub expected_public_caps: Option<CapabilitySet>,
    #[serde(skip)]
    pub expected_name: Option<FnName>,
}

impl Default for FnNode {
    fn default() -> Self {
        Self {
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
}

impl FnNode {
    /// Merge another callgraph entry for the same function into this one.
    pub fn rvs_merge_M(&mut self, other: &Self) {
        self.calls.extend(other.calls.iter().cloned());
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
        self.sources.extend(other.sources.iter().cloned());
        self.report_caps = self.report_caps.clone().or(other.report_caps.clone());
        self.report_line_count = self.report_line_count.or(other.report_line_count);
        self.allows_dead_code |= other.allows_dead_code;
        self.is_synthetic = self.is_synthetic && other.is_synthetic;
    }

    pub(crate) fn rvs_set_expected_public_caps_M(&mut self, caps: CapabilitySet) {
        self.expected_public_caps = Some(caps);
    }

    pub(crate) fn rvs_clear_expected_public_caps_M(&mut self) {
        self.expected_public_caps = None;
    }

    pub(crate) fn rvs_set_expected_name_M(&mut self, name: FnName) {
        self.expected_name = Some(name);
    }

    pub(crate) fn rvs_clear_expected_name_M(&mut self) {
        self.expected_name = None;
    }
}

pub type FnBehavior = FnNode;

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
    pub(crate) fn rvs_contains_key(&self, path: &DefPath) -> bool {
        self.nodes.contains_key(path)
    }

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

    pub(crate) fn rvs_merge_from_M(&mut self, other: Self) {
        for (path, node) in other.nodes {
            self.rvs_merge_node_M(path, node);
        }
    }

    #[cfg(test)]
    pub(crate) fn rvs_len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn rvs_is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(crate) fn rvs_expected_public_caps_map(&self) -> BTreeMap<DefPath, CapabilitySet> {
        self.nodes
            .iter()
            .filter_map(|(path, node)| {
                node.expected_public_caps
                    .clone()
                    .map(|caps| (path.clone(), caps))
            })
            .collect()
    }
}

/// Parse serialized callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json_S(json: &str) -> Result<FnGraph, String> {
    let graph: FnGraph =
        serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))?;
    for (path, node) in graph.rvs_iter() {
        if path.rvs_as_str().is_empty() {
            return Err("invalid callgraph JSON: function path is empty".into());
        }
        for callee in &node.calls {
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
    fn test_20260703_graph_mutation_helpers() {
        let mut graph = FnGraph::rvs_new();
        let path = DefPath::from("demo::rvs_run");
        graph.rvs_insert_M(path.clone(), FnNode::default());
        graph
            .rvs_get_mut_M(&path)
            .expect("graph should contain inserted node")
            .rvs_set_expected_public_caps_M(CapabilitySet::rvs_new());
        graph
            .rvs_get_mut_M(&path)
            .expect("graph should contain inserted node")
            .rvs_set_expected_name_M(FnName::from("rvs_run"));
        let inferred = graph.rvs_expected_public_caps_map();
        let output = format!(
            "contains={}\ninferred={}\nempty={}\nkeys={}\nname={}\n",
            graph.rvs_contains_key(&path),
            inferred.contains_key(&path),
            graph.rvs_is_empty(),
            graph.rvs_keys().count(),
            graph
                .rvs_get("demo::rvs_run")
                .and_then(|node| node.expected_name.as_ref())
                .map(FnName::rvs_as_str)
                .unwrap_or("")
        );
        rvs_snapshot_BIS("test_20260703_graph_mutation_helpers", &output);

        graph
            .rvs_get_mut_M(&path)
            .expect("graph should still contain inserted node")
            .rvs_clear_expected_public_caps_M();
        graph
            .rvs_get_mut_M(&path)
            .expect("graph should still contain inserted node")
            .rvs_clear_expected_name_M();
        let cleared = graph.rvs_expected_public_caps_map();

        assert!(graph.rvs_contains_key(&path));
        assert!(inferred.contains_key(&path));
        assert!(!graph.rvs_is_empty());
        assert_eq!(graph.rvs_keys().count(), 1);
        assert!(!cleared.contains_key(&path));
        assert!(
            graph
                .rvs_get("demo::rvs_run")
                .and_then(|node| node.expected_name.as_ref())
                .is_none()
        );
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
    fn test_20260703_graph_iter_mut_helper() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_a"), FnNode::default());
        for (_, node) in graph.rvs_iter_mut_M() {
            node.rvs_set_expected_name_M(FnName::from("rvs_a"));
        }
        let output = format!(
            "name={}\n",
            graph
                .rvs_get("demo::rvs_a")
                .and_then(|node| node.expected_name.as_ref())
                .map(FnName::rvs_as_str)
                .unwrap_or("")
        );
        rvs_snapshot_BIS("test_20260703_graph_iter_mut_helper", &output);

        assert_eq!(
            graph
                .rvs_get("demo::rvs_a")
                .and_then(|node| node.expected_name.as_ref())
                .map(FnName::rvs_as_str),
            Some("rvs_a")
        );
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
