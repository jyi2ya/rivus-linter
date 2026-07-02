use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityFacts;
use crate::symbols::{DefPath, FnName};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FnBehavior {
    pub calls: BTreeSet<DefPath>,
    #[serde(flatten)]
    pub facts: CapabilityFacts,
    #[serde(default)]
    pub is_trait_impl: bool,
    #[serde(default)]
    pub is_test: bool,
}

impl FnBehavior {
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
        self.is_trait_impl |= other.is_trait_impl;
        self.is_test |= other.is_test;
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FnReportEntry {
    pub name: FnName,
    pub caps: String,
    pub lines: usize,
    pub is_test: bool,
    pub allows_dead_code: bool,
}

/// Parse serialized callgraph JSON into shared callgraph records.
pub fn rvs_parse_callgraph_json_S(json: &str) -> Result<BTreeMap<DefPath, FnBehavior>, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid callgraph JSON: {e}"))
}

/// Parse serialized function report JSON into shared report records.
pub fn rvs_parse_report_json_S(json: &str) -> Result<Vec<FnReportEntry>, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid report JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    #[test]
    fn test_20260609_parse_callgraph_valid_json() {
        let json = r#"{
            "my_crate::rvs_add": {
                "calls": ["my_crate::rvs_helper"],
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
        assert_eq!(result.len(), 2);

        let add_behavior = result
            .get("my_crate::rvs_add")
            .expect("should find rvs_add");
        assert!(add_behavior.calls.contains("my_crate::rvs_helper"));

        let write_behavior = result
            .get("my_crate::rvs_write_BI")
            .expect("should find rvs_write_BI");
        assert!(write_behavior.calls.contains("std::fs::write"));
    }

    #[test]
    fn test_20260609_parse_callgraph_invalid_json() {
        let json = "this is not json at all";
        let result = rvs_parse_callgraph_json_S(json);
        rvs_snapshot_BIS(
            "test_20260609_parse_callgraph_invalid_json",
            &format!("{result:?}"),
        );
        assert!(result.is_err());
    }
}
