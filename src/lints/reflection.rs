use rustc_hir::{self, Body};
use rustc_lint::{LateContext, LintContext};

use super::RVS_REFLECTION_USAGE;
use super::msg::Msg;
use super::utils::{CallTarget, rvs_is_reflection_S, rvs_resolve_call, rvs_walk_closures};

/// Walk function body looking for reflection usage (type_name, type_id, Any).
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>) {
    rvs_walk_closures(cx.tcx, body.value, |expr| {
        let Some(observation) = rvs_resolve_call(cx, expr) else {
            return;
        };
        let path = match &observation.target {
            CallTarget::Resolved {
                def_path,
                def_kind:
                    rustc_hir::def::DefKind::Fn
                    | rustc_hir::def::DefKind::AssocFn
                    | rustc_hir::def::DefKind::Variant,
            } => def_path,
            CallTarget::UnresolvedPath { path } => path,
            CallTarget::Resolved { .. } => return,
        };
        if rvs_is_reflection_S(path) {
            cx.emit_span_lint(
                RVS_REFLECTION_USAGE,
                observation.span,
                Msg::rvs_new(observation.span, "reflection — use trait dispatch instead"),
            );
        }
    });
}
