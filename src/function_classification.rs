use std::collections::BTreeSet;

use crate::artifacts::FnNode;
use crate::symbols::{CrateName, DefPath, DefPathPrefix, FnName, RelativeFnPath};

#[derive(Debug)]
pub(crate) struct LocalScope {
    prefixes: Vec<DefPathPrefix>,
    root_main_paths: BTreeSet<DefPath>,
}

impl LocalScope {
    pub(crate) fn rvs_new(local_crate_names: &BTreeSet<CrateName>) -> Self {
        let prefixes: Vec<_> = local_crate_names
            .iter()
            .map(CrateName::rvs_prefix)
            .collect();
        let root_main_paths = prefixes
            .iter()
            .map(|prefix| prefix.rvs_join_name(&FnName::rvs_new("main")))
            .collect();
        Self {
            prefixes,
            root_main_paths,
        }
    }

    pub(crate) fn rvs_contains(&self, def_path: &DefPath) -> bool {
        self.prefixes
            .iter()
            .any(|prefix| def_path.rvs_starts_with(prefix))
    }

    pub(crate) fn rvs_is_root_main(&self, def_path: &DefPath) -> bool {
        self.root_main_paths.contains(def_path)
    }

    pub(crate) fn rvs_local_relative_path(&self, def_path: &DefPath) -> Option<RelativeFnPath> {
        self.prefixes
            .iter()
            .find_map(|prefix| def_path.rvs_strip_prefix(prefix))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FunctionClassification {
    is_local: bool,
    is_root_main: bool,
    is_test: bool,
    is_trait_impl: bool,
    is_port_method: bool,
    has_source: bool,
    is_generated_helper: bool,
}

impl FunctionClassification {
    pub(crate) fn rvs_new(scope: &LocalScope, def_path: &DefPath, node: &FnNode) -> Self {
        Self {
            is_local: scope.rvs_contains(def_path),
            is_root_main: scope.rvs_is_root_main(def_path),
            is_test: node.is_test,
            is_trait_impl: node.is_trait_impl,
            is_port_method: node.facts.is_port_method,
            has_source: !node.sources.is_empty(),
            is_generated_helper: rvs_is_generated_snafu_helper(def_path),
        }
    }

    pub(crate) fn rvs_is_contract_enforced(self) -> bool {
        self.is_local && !self.is_root_main && !self.is_test && !self.is_trait_impl
    }

    pub(crate) fn rvs_is_offline_checked(self) -> bool {
        self.is_local
            && !self.is_root_main
            && !self.is_test
            && (!self.is_trait_impl || self.is_port_method)
            && self.has_source
            && !self.is_generated_helper
    }

    pub(crate) fn rvs_is_report_candidate(self) -> bool {
        self.is_local && (!self.is_trait_impl || self.is_port_method)
    }

    pub(crate) fn rvs_is_strip_candidate(self) -> bool {
        self.is_local && !self.is_trait_impl
    }
}

fn rvs_is_generated_snafu_helper(def_path: &DefPath) -> bool {
    let path = def_path.rvs_as_str();
    let fn_name = def_path.rvs_fn_name();
    matches!(fn_name.rvs_as_str(), "build" | "fail")
        && path.split("::").any(|segment| segment.ends_with("Snafu"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FnSource;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260711_function_classification_policy_matrix() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let cases = [
            ("ordinary", "demo::rvs_run", false, false, false, true),
            ("root_main", "demo::main", false, false, false, false),
            (
                "external",
                "dependency::rvs_run",
                false,
                false,
                false,
                false,
            ),
            ("test", "demo::rvs_test", true, false, false, true),
            (
                "trait_impl",
                "demo::Adapter::rvs_run@demo::Trait",
                false,
                true,
                false,
                true,
            ),
            (
                "port_impl",
                "demo::Adapter::rvs_run@demo::Client",
                false,
                true,
                true,
                true,
            ),
            (
                "snafu",
                "demo::ErrorSnafu::build",
                false,
                false,
                false,
                true,
            ),
        ];
        let mut output = String::new();
        for (name, path, is_test, is_trait_impl, is_port_method, has_source) in cases {
            let mut node = FnNode {
                is_test,
                is_trait_impl,
                ..FnNode::default()
            };
            node.facts.is_port_method = is_port_method;
            if has_source {
                node.sources
                    .insert(FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2));
            }
            let classification =
                FunctionClassification::rvs_new(&scope, &DefPath::from(path), &node);
            output.push_str(&format!(
                "{name}: contract={} offline={} report={} strip={}\n",
                classification.rvs_is_contract_enforced(),
                classification.rvs_is_offline_checked(),
                classification.rvs_is_report_candidate(),
                classification.rvs_is_strip_candidate(),
            ));
        }
        rvs_snapshot_BIS(
            "test_20260711_function_classification_policy_matrix",
            &output,
        );
    }
}
