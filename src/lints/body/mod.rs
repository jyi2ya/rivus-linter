pub(crate) mod catch_unwind;
pub(crate) mod collector;
pub(crate) mod debug_assert;
pub(crate) mod empty_fn;
pub(crate) mod error_swallow;
pub(crate) mod macro_expansion;
pub(crate) mod reflection;
pub(crate) mod spawn;
pub(crate) mod stub_macro;

use super::utils::CallTarget;

pub(crate) use collector::{BodyFacts, rvs_collect_body_facts_M};

fn rvs_path_lint_callable(target: &CallTarget) -> Option<&str> {
    match target {
        CallTarget::Resolved {
            def_path,
            def_kind:
                rustc_hir::def::DefKind::Fn
                | rustc_hir::def::DefKind::AssocFn
                | rustc_hir::def::DefKind::Variant,
            ..
        } => Some(def_path.rvs_as_str()),
        CallTarget::UnresolvedPath { path } => Some(path),
        CallTarget::UnresolvedMethod { .. } | CallTarget::Resolved { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::DefPath;
    use crate::test_support::rvs_snapshot_BIS;
    use rustc_hir::def::DefKind;

    #[test]
    fn test_20260714_path_lint_callable_table() {
        let resolved = CallTarget::Resolved {
            def_path: DefPath::from("demo::rvs_run"),
            def_kind: DefKind::Fn,
            crate_id: 1,
        };
        let unresolved_path = CallTarget::UnresolvedPath {
            path: "demo::rvs_path".to_string(),
        };
        let unresolved_method = CallTarget::UnresolvedMethod {
            name: "rvs_method".to_string(),
        };
        let output = format!(
            "resolved={:?}\npath={:?}\nmethod={:?}\n",
            rvs_path_lint_callable(&resolved),
            rvs_path_lint_callable(&unresolved_path),
            rvs_path_lint_callable(&unresolved_method),
        );
        rvs_snapshot_BIS("test_20260714_path_lint_callable_table", &output);

        assert_eq!(rvs_path_lint_callable(&resolved), Some("demo::rvs_run"));
        assert_eq!(
            rvs_path_lint_callable(&unresolved_path),
            Some("demo::rvs_path")
        );
        assert_eq!(rvs_path_lint_callable(&unresolved_method), None);
    }
}
