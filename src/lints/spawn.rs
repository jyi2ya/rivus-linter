use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_SPAWN_WARNING;
use super::msg::Msg;
use super::utils::{rvs_def_path, rvs_is_spawn_S, rvs_qp, rvs_walk_closures};

/// Walk function body looking for spawn calls outside of tests.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, is_test: bool) {
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
                        let fp = rvs_def_path(cx, did);
                        if !is_test && rvs_is_spawn_S(&fp) {
                            cx.emit_span_lint(
                                RVS_SPAWN_WARNING,
                                e.span,
                                Msg::new(
                                    e.span,
                                    format!("spawn: {fp} — use structured concurrency"),
                                ),
                            );
                        }
                    }
                } else {
                    let ps = rvs_qp(q);
                    if !is_test && rvs_is_spawn_S(&ps) {
                        cx.emit_span_lint(
                            RVS_SPAWN_WARNING,
                            e.span,
                            Msg::new(e.span, format!("spawn: {ps} — use structured concurrency")),
                        );
                    }
                }
            }
        }
        ExprKind::MethodCall(..) => {
            let owner = e.hir_id.owner.def_id;
            let tck = cx.tcx.typeck(owner);
            if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                let fp = rvs_def_path(cx, did);
                if !is_test && rvs_is_spawn_S(&fp) {
                    cx.emit_span_lint(
                        RVS_SPAWN_WARNING,
                        e.span,
                        Msg::new(e.span, format!("spawn: {fp}")),
                    );
                }
            }
        }
        _ => {}
    });
}
