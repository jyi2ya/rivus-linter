use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_CATCH_UNWIND;
use super::msg::Msg;
use super::utils::{CallSyntax, CallTarget, rvs_resolve_call, rvs_walk_closures};

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>) {
    rvs_walk_closures(cx.tcx, body.value, |expr| match &expr.kind {
        ExprKind::MethodCall(p, ..) => {
            let n = p.ident.name.as_str();
            if n == "catch_unwind" {
                cx.emit_span_lint(
                    RVS_CATCH_UNWIND,
                    expr.span,
                    Msg::rvs_new(expr.span, "catch_unwind — fix panic source instead"),
                );
            }
        }
        _ => {
            let Some(observation) = rvs_resolve_call(cx, expr) else {
                return;
            };
            if observation.syntax != CallSyntax::Function {
                return;
            }
            let CallTarget::Resolved {
                def_path,
                def_kind:
                    rustc_hir::def::DefKind::Fn
                    | rustc_hir::def::DefKind::AssocFn
                    | rustc_hir::def::DefKind::Variant,
            } = &observation.target
            else {
                return;
            };
            if def_path.rsplit("::").next() == Some("catch_unwind") {
                cx.emit_span_lint(
                    RVS_CATCH_UNWIND,
                    observation.span,
                    Msg::rvs_new(observation.span, "catch_unwind — fix panic source instead"),
                );
            }
        }
    });
}
