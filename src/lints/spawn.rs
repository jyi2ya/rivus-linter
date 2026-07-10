use rustc_hir::{self, Body};
use rustc_lint::{LateContext, LintContext};

use super::RVS_SPAWN_WARNING;
use super::msg::Msg;
use super::utils::{CallSyntax, CallTarget, rvs_is_spawn_S, rvs_resolve_call, rvs_walk_closures};

/// Walk function body looking for spawn calls outside of tests.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, is_test: bool) {
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
        if !is_test && rvs_is_spawn_S(path) {
            let message = match observation.syntax {
                CallSyntax::Function => {
                    format!("spawn: {path} — use structured concurrency")
                }
                CallSyntax::Method => format!("spawn: {path}"),
            };
            cx.emit_span_lint(
                RVS_SPAWN_WARNING,
                observation.span,
                Msg::rvs_new(observation.span, message),
            );
        }
    });
}
