use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_CATCH_UNWIND;
use super::msg::Msg;
use super::utils::rvs_walk_closures;

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>) {
    rvs_walk_closures(cx.tcx, body.value, |e| match &e.kind {
        ExprKind::Call(func, _) => {
            if let ExprKind::Path(ref q) = func.kind {
                if let rustc_hir::def::Res::Def(k, did) = cx.qpath_res(q, func.hir_id) {
                    if matches!(
                        k,
                        rustc_hir::def::DefKind::Fn
                            | rustc_hir::def::DefKind::AssocFn
                            | rustc_hir::def::DefKind::Variant
                    ) {
                        let fp = crate::lints::utils::rvs_def_path(cx, did);
                        let cn = fp.rsplit("::").next().unwrap_or(&fp);
                        if cn == "catch_unwind" {
                            cx.emit_span_lint(
                                RVS_CATCH_UNWIND,
                                e.span,
                                Msg::new(e.span, "catch_unwind — fix panic source instead"),
                            );
                        }
                    }
                }
            }
        }
        ExprKind::MethodCall(p, ..) => {
            let n = p.ident.name.as_str();
            if n == "catch_unwind" {
                cx.emit_span_lint(
                    RVS_CATCH_UNWIND,
                    e.span,
                    Msg::new(e.span, "catch_unwind — fix panic source instead"),
                );
            }
        }
        _ => {}
    });
}
