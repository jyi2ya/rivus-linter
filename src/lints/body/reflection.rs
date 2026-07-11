use rustc_lint::{LateContext, LintContext};

use super::super::RVS_REFLECTION_USAGE;
use super::super::msg::Msg;
use super::super::utils::{CallTarget, rvs_is_reflection_S};
use super::BodyFacts;

/// Walk function body looking for reflection usage (type_name, type_id, Any).
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        let path = match &observation.target {
            CallTarget::Resolved {
                def_path,
                def_kind:
                    rustc_hir::def::DefKind::Fn
                    | rustc_hir::def::DefKind::AssocFn
                    | rustc_hir::def::DefKind::Variant,
            } => def_path,
            CallTarget::UnresolvedPath { path } => path,
            CallTarget::UnresolvedMethod { .. } => continue,
            CallTarget::Resolved { .. } => continue,
        };
        if rvs_is_reflection_S(path) {
            cx.emit_span_lint(
                RVS_REFLECTION_USAGE,
                observation.span,
                Msg::rvs_new(observation.span, "reflection — use trait dispatch instead"),
            );
        }
    }
}
