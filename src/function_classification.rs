use std::collections::BTreeSet;

use crate::artifacts::FnNode;
use crate::symbols::{CrateName, DefPath, DefPathPrefix, TraitMethodIdentity};

#[derive(Debug)]
pub(crate) struct LocalScope {
    prefixes: Vec<DefPathPrefix>,
}

impl LocalScope {
    pub(crate) fn rvs_new(local_crate_names: &BTreeSet<CrateName>) -> Self {
        let prefixes: Vec<_> = local_crate_names
            .iter()
            .map(CrateName::rvs_prefix)
            .collect();
        Self { prefixes }
    }

    pub(crate) fn rvs_contains(&self, def_path: &DefPath) -> bool {
        self.rvs_contains_str(def_path.rvs_as_str())
    }

    pub(crate) fn rvs_contains_str(&self, def_path: &str) -> bool {
        self.prefixes
            .iter()
            .any(|prefix| def_path.starts_with(prefix.rvs_as_str()))
            || TraitMethodIdentity::rvs_parse(def_path)
                .is_some_and(|identity| self.rvs_contains(&identity.rvs_trait_method_path()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FunctionClassification {
    is_local: bool,
    is_entrypoint: bool,
    is_test: bool,
    is_trait_impl: bool,
    is_port_method: bool,
    has_source: bool,
}

impl FunctionClassification {
    pub(crate) fn rvs_new(scope: &LocalScope, def_path: &DefPath, node: &FnNode) -> Self {
        Self {
            is_local: scope.rvs_contains(def_path),
            is_entrypoint: node.is_entrypoint,
            is_test: node.is_test,
            is_trait_impl: node.is_trait_impl,
            is_port_method: node.facts.is_port_method,
            has_source: !node.sources.is_empty(),
        }
    }

    pub(crate) fn rvs_new_for_crate(
        scope: &LocalScope,
        def_path: &DefPath,
        node: &FnNode,
        crate_id: u64,
    ) -> Self {
        debug_assert!(crate_id > 0, "stable crate id is nonzero");
        let mut classification = Self::rvs_new(scope, def_path, node);
        classification.is_entrypoint = node.rvs_is_entrypoint_for_crate(crate_id);
        classification
    }

    pub(crate) fn rvs_is_contract_enforced(self) -> bool {
        self.is_local
            && !self.is_entrypoint
            && !self.is_test
            && !self.is_trait_impl
            && self.has_source
    }

    pub(crate) fn rvs_is_offline_checked(self) -> bool {
        self.is_local
            && !self.is_entrypoint
            && !self.is_test
            && (!self.is_trait_impl || self.is_port_method)
            && self.has_source
    }

    pub(crate) fn rvs_is_report_candidate(self) -> bool {
        self.is_local && (!self.is_trait_impl || self.is_port_method)
    }

    pub(crate) fn rvs_is_trait_vote_outlier_candidate(self) -> bool {
        self.is_local && self.is_trait_impl && !self.is_port_method && self.has_source
    }

    pub(crate) fn rvs_is_strip_candidate(self) -> bool {
        self.is_local && !self.is_trait_impl
    }
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
            (
                "ordinary",
                "demo::rvs_run",
                false,
                false,
                false,
                false,
                true,
            ),
            ("entry_main", "demo::main", false, false, false, true, true),
            (
                "library_main",
                "demo::main",
                false,
                false,
                false,
                false,
                true,
            ),
            (
                "external",
                "dependency::rvs_run",
                false,
                false,
                false,
                false,
                false,
            ),
            ("test", "demo::rvs_test", true, false, false, false, true),
            (
                "trait_impl",
                "demo::Adapter::rvs_run@demo::Trait",
                false,
                true,
                false,
                false,
                true,
            ),
            (
                "port_impl",
                "demo::Adapter::rvs_run@demo::Client",
                false,
                true,
                true,
                false,
                true,
            ),
            (
                "generated_snafu",
                "demo::ErrorSnafu::build",
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "user_snafu",
                "demo::UserSnafu::build",
                false,
                false,
                false,
                false,
                true,
            ),
        ];
        let mut output = String::new();
        for (name, path, is_test, is_trait_impl, is_port_method, is_entrypoint, has_source) in cases
        {
            let mut node = FnNode {
                is_test,
                is_trait_impl,
                is_entrypoint,
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
                "{name}: contract={} offline={} report={} outlier={} strip={}\n",
                classification.rvs_is_contract_enforced(),
                classification.rvs_is_offline_checked(),
                classification.rvs_is_report_candidate(),
                classification.rvs_is_trait_vote_outlier_candidate(),
                classification.rvs_is_strip_candidate(),
            ));
        }
        rvs_snapshot_BIS(
            "test_20260711_function_classification_policy_matrix",
            &output,
        );
    }

    #[test]
    fn test_20260712_local_scope_matches_typed_and_borrowed_paths() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([
            CrateName::from("demo"),
            CrateName::from("cargo-rivus"),
        ]));
        let cases = [
            "demo::rvs_run",
            "cargo_rivus::rvs_check",
            "dependency::rvs_run",
            "demonstration::rvs_run",
        ];
        let mut output = String::new();
        for path in cases {
            let typed = scope.rvs_contains(&DefPath::from(path));
            let borrowed = scope.rvs_contains_str(path);
            output.push_str(&format!("{path}: typed={typed} borrowed={borrowed}\n"));
            assert_eq!(typed, borrowed);
        }
        rvs_snapshot_BIS(
            "test_20260712_local_scope_matches_typed_and_borrowed_paths",
            &output,
        );
    }

    #[test]
    fn test_20260715_local_trait_impl_for_external_type_stays_local() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let path = DefPath::from(
            "std::fs::File{impl#7374643a3a66733a3a46696c654064656d6f3a3a46696c65436c69656e74}::rvs_touch_P@demo::FileClient",
        );
        let mut node = FnNode {
            is_trait_impl: true,
            ..FnNode::default()
        };
        node.facts.is_port_method = true;
        node.sources
            .insert(FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2));

        let classification = FunctionClassification::rvs_new(&scope, &path, &node);
        let output = format!(
            "scope_contains={}\noffline={}\nreport={}\n",
            scope.rvs_contains(&path),
            classification.rvs_is_offline_checked(),
            classification.rvs_is_report_candidate(),
        );
        rvs_snapshot_BIS(
            "test_20260715_local_trait_impl_for_external_type_stays_local",
            &output,
        );

        assert!(scope.rvs_contains(&path));
        assert!(classification.rvs_is_offline_checked());
        assert!(classification.rvs_is_report_candidate());
    }

    #[test]
    fn test_20260716_function_classification_uses_target_entrypoint_identity() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let path = DefPath::from("demo::main");
        let mut node = FnNode::default();
        node.sources
            .insert(FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2));
        node.production_crate_ids.extend([1, 2]);
        node.entrypoint_crate_ids.insert(2);

        let library = FunctionClassification::rvs_new_for_crate(&scope, &path, &node, 1);
        let binary = FunctionClassification::rvs_new_for_crate(&scope, &path, &node, 2);
        let output = format!(
            "library_contract={}\nlibrary_offline={}\nbinary_contract={}\nbinary_offline={}\n",
            library.rvs_is_contract_enforced(),
            library.rvs_is_offline_checked(),
            binary.rvs_is_contract_enforced(),
            binary.rvs_is_offline_checked(),
        );
        rvs_snapshot_BIS(
            "test_20260716_function_classification_uses_target_entrypoint_identity",
            &output,
        );

        assert!(library.rvs_is_contract_enforced());
        assert!(library.rvs_is_offline_checked());
        assert!(!binary.rvs_is_contract_enforced());
        assert!(!binary.rvs_is_offline_checked());
    }
}
