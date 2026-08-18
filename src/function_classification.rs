use std::collections::BTreeSet;

use crate::artifacts::{CrateProvenance, FnGraph, FnNode, FunctionIdentity};
use crate::symbols::{
    CrateName, DefPath, DefPathPrefix, TraitMethodIdentity, rvs_strip_identity_markers,
};

#[derive(Debug)]
pub(crate) struct LocalScope {
    prefixes: Vec<DefPathPrefix>,
    primary_crate_ids: BTreeSet<u64>,
    dependency_crate_ids: BTreeSet<u64>,
}

impl LocalScope {
    pub(crate) fn rvs_new(local_crate_names: &BTreeSet<CrateName>) -> Self {
        let prefixes: Vec<_> = local_crate_names
            .iter()
            .map(CrateName::rvs_prefix)
            .collect();
        Self {
            prefixes,
            primary_crate_ids: BTreeSet::new(),
            dependency_crate_ids: BTreeSet::new(),
        }
    }

    pub(crate) fn rvs_for_graph(local_crate_names: &BTreeSet<CrateName>, graph: &FnGraph) -> Self {
        let mut scope = Self::rvs_new(local_crate_names);
        for node in graph.nodes.values() {
            match node.crate_provenance {
                CrateProvenance::PrimaryPackage => {
                    scope.primary_crate_ids.insert(node.crate_id);
                }
                CrateProvenance::Dependency => {
                    scope.dependency_crate_ids.insert(node.crate_id);
                }
                CrateProvenance::LegacyUnknown => {}
            }
        }
        debug_assert!(
            scope
                .primary_crate_ids
                .is_disjoint(&scope.dependency_crate_ids)
        );
        scope
    }

    pub(crate) fn rvs_contains(&self, def_path: &DefPath) -> bool {
        self.rvs_contains_str(def_path.rvs_as_str())
    }

    pub(crate) fn rvs_contains_str(&self, def_path: &str) -> bool {
        let user_path = rvs_strip_identity_markers(def_path);
        self.prefixes
            .iter()
            .any(|prefix| user_path.starts_with(prefix.rvs_as_str()))
            || TraitMethodIdentity::rvs_parse(user_path.as_ref())
                .is_some_and(|identity| self.rvs_contains(&identity.rvs_trait_method_path()))
    }

    pub(crate) fn rvs_contains_target(
        &self,
        def_path: &DefPath,
        provenance: CrateProvenance,
    ) -> bool {
        match provenance {
            CrateProvenance::PrimaryPackage => true,
            CrateProvenance::Dependency => false,
            CrateProvenance::LegacyUnknown => self.rvs_contains(def_path),
        }
    }

    pub(crate) fn rvs_contains_identity(&self, identity: &FunctionIdentity) -> bool {
        if identity.crate_id == 0 {
            return self.rvs_contains(&identity.def_path);
        }
        if self.primary_crate_ids.contains(&identity.crate_id) {
            return true;
        }
        if self.dependency_crate_ids.contains(&identity.crate_id) {
            return false;
        }
        self.rvs_contains(&identity.def_path)
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
    /// Build the aggregate compatibility view used by identity-less legacy graphs.
    /// Targeted graphs retain the graph's deduplicated aggregate roles, while locality is true
    /// only when at least one selected target belongs to the primary package.
    pub(crate) fn rvs_new(scope: &LocalScope, def_path: &DefPath, node: &FnNode) -> Self {
        Self {
            is_local: node.complete && scope.rvs_contains_target(def_path, node.crate_provenance),
            is_entrypoint: node.is_entrypoint,
            is_test: node.is_test,
            is_trait_impl: node.is_trait_impl,
            is_port_method: node.facts.is_port_method,
            has_source: !node.sources.is_empty(),
        }
    }

    pub(crate) const fn rvs_with_port(mut self, is_port_method: bool) -> Self {
        self.is_port_method = is_port_method;
        self
    }

    pub(crate) const fn rvs_is_contract_enforced(self) -> bool {
        self.is_local
            && !self.is_entrypoint
            && !self.is_test
            && !self.is_trait_impl
            && self.has_source
    }

    pub(crate) const fn rvs_is_offline_checked(self) -> bool {
        self.is_local
            && !self.is_entrypoint
            && !self.is_test
            && (!self.is_trait_impl || self.is_port_method)
            && self.has_source
    }

    pub(crate) const fn rvs_is_report_candidate(self) -> bool {
        self.is_local && (!self.is_trait_impl || self.is_port_method)
    }

    pub(crate) const fn rvs_is_trait_vote_outlier_candidate(self) -> bool {
        self.is_local && self.is_trait_impl && !self.is_port_method && self.has_source
    }

    pub(crate) const fn rvs_is_strip_candidate(self) -> bool {
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
    fn test_20260801_415_generated_identity_retains_canonical_crate_prefix() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let mut local_parts = vec!["demo".to_string(), "rvs_generated".to_string()];
        crate::symbols::rvs_attach_generated_definition_marker_M(
            &mut local_parts,
            "657870616e73696f6e",
        );
        let mut std_parts = vec!["std".to_string(), "rvs_generated".to_string()];
        crate::symbols::rvs_attach_generated_definition_marker_M(
            &mut std_parts,
            "657870616e73696f6e",
        );
        let local = DefPath::rvs_new(local_parts.join("::"));
        let std = DefPath::rvs_new(std_parts.join("::"));
        let output = format!(
            "local_raw={}\nlocal_user={}\nlocal_function={}\nlocal_classified={}\nstd_raw={}\nstd_user={}\nstd_function={}\nstd_classified={}\n",
            local.rvs_as_str(),
            local,
            local.rvs_fn_name_str(),
            scope.rvs_contains(&local),
            std.rvs_as_str(),
            std,
            std.rvs_fn_name_str(),
            crate::callgraph::rvs_is_std_like_def_path(std.rvs_as_str()),
        );
        rvs_snapshot_BIS(
            "test_20260801_415_generated_identity_retains_canonical_crate_prefix",
            &output,
        );

        assert!(local.rvs_as_str().starts_with("demo::"));
        assert_eq!(local.rvs_user_path(), "demo::rvs_generated");
        assert_eq!(local.rvs_fn_name_str(), "rvs_generated");
        assert!(scope.rvs_contains(&local));
        assert!(std.rvs_as_str().starts_with("std::"));
        assert_eq!(std.rvs_user_path(), "std::rvs_generated");
        assert_eq!(std.rvs_fn_name_str(), "rvs_generated");
        assert!(crate::callgraph::rvs_is_std_like_def_path(std.rvs_as_str()));
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
        let source = FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2);
        node.sources.insert(source.clone());
        node.is_production = true;
        node.is_entrypoint = true;
        node.crate_id = 2;
        node.crate_provenance = crate::artifacts::CrateProvenance::PrimaryPackage;

        let binary = FunctionClassification::rvs_new(&scope, &path, &node);
        let output = format!(
            "binary_contract={}\nbinary_offline={}\n",
            binary.rvs_is_contract_enforced(),
            binary.rvs_is_offline_checked(),
        );
        rvs_snapshot_BIS(
            "test_20260716_function_classification_uses_target_entrypoint_identity",
            &output,
        );

        assert!(!binary.rvs_is_contract_enforced());
        assert!(!binary.rvs_is_offline_checked());
    }

    #[test]
    fn test_20260726_function_classification_uses_target_facts_and_sources() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let path = DefPath::from("demo::Worker::rvs_run@demo::Runner");
        let source = FnSource::rvs_new("/workspace/src/lib.rs".into(), 1, 2);
        let mut node = FnNode {
            is_trait_impl: true,
            sources: BTreeSet::from([source]),
            crate_id: 10,
            crate_provenance: crate::artifacts::CrateProvenance::PrimaryPackage,
            ..FnNode::default()
        };
        node.facts.is_port_method = false;

        let ordinary_impl = FunctionClassification::rvs_new(&scope, &path, &node);
        let output = format!(
            "ordinary_impl: offline={} report={} outlier={}\n",
            ordinary_impl.rvs_is_offline_checked(),
            ordinary_impl.rvs_is_report_candidate(),
            ordinary_impl.rvs_is_trait_vote_outlier_candidate(),
        );
        rvs_snapshot_BIS(
            "test_20260726_function_classification_uses_target_facts_and_sources",
            &output,
        );

        assert!(ordinary_impl.rvs_is_trait_vote_outlier_candidate());
        assert!(!ordinary_impl.rvs_is_offline_checked());
    }

    #[test]
    fn test_20260729_per_target_classification_does_not_inherit_roles() {
        let scope = LocalScope::rvs_new(&BTreeSet::from([CrateName::from("demo")]));
        let path = DefPath::from("demo::rvs_shared");
        let production_source = FnSource::rvs_new("/workspace/src/lib.rs".into(), 7, 17);
        let mut node = FnNode {
            is_entrypoint: true,
            is_test: true,
            is_trait_impl: true,
            is_production: true,
            sources: BTreeSet::from([production_source.clone()]),
            crate_id: 10,
            crate_provenance: crate::artifacts::CrateProvenance::PrimaryPackage,
            ..FnNode::default()
        };
        node.sources.insert(production_source);

        let production = FunctionClassification::rvs_new(&scope, &path, &node);
        let output = format!(
            "entry_test_trait_impl: contract={} offline={} report={} outlier={} strip={}\n",
            production.rvs_is_contract_enforced(),
            production.rvs_is_offline_checked(),
            production.rvs_is_report_candidate(),
            production.rvs_is_trait_vote_outlier_candidate(),
            production.rvs_is_strip_candidate(),
        );
        rvs_snapshot_BIS(
            "test_20260729_per_target_classification_does_not_inherit_roles",
            &output,
        );

        assert!(!production.rvs_is_contract_enforced());
        assert!(!production.rvs_is_offline_checked());
    }
}
