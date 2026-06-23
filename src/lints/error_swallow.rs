use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_ERROR_SWALLOW;
use super::msg::Msg;
use super::utils::{ERROR_SWALLOW_METHODS, rvs_walk_closures};

/// Walk function body looking for `.ok()` and `.unwrap_or_default()` calls.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>) {
    rvs_walk_closures(cx.tcx, body.value, |e| {
        if let ExprKind::MethodCall(p, ..) = &e.kind {
            let n = p.ident.name.as_str();
            if ERROR_SWALLOW_METHODS.contains(&n) {
                cx.emit_span_lint(
                    RVS_ERROR_SWALLOW,
                    e.span,
                    Msg::new(e.span, format!(".{n}() swallows errors")),
                );
            }
        }
    });
}
