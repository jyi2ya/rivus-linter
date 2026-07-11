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
        } => Some(def_path),
        CallTarget::UnresolvedPath { path } => Some(path),
        CallTarget::UnresolvedMethod { .. } | CallTarget::Resolved { .. } => None,
    }
}
