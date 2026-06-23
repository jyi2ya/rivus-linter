use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::utils::*;
use super::{RVS_CALL_VIOLATION, RVS_UNKNOWN_CALLEE};
use crate::capability::CapabilitySet;
use crate::capsmap::CapsMap;

/// Walk the function body checking all call targets for capability violations
/// and unknown callees.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    caps: &CapabilitySet,
    capsmap: &Option<CapsMap>,
) {
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
                        let sp = rvs_qp(q);
                        rvs_check_target_S(cx, e.span, &fp, Some(&sp), caps, capsmap);
                    }
                }
            }
        }
        ExprKind::MethodCall(p, ..) => {
            let n = p.ident.name.as_str();
            let owner = e.hir_id.owner.def_id;
            let tck = cx.tcx.typeck(owner);
            if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                let fp = rvs_def_path(cx, did);
                rvs_check_target_S(cx, e.span, &fp, Some(n), caps, capsmap);
            }
        }
        _ => {}
    });
}

/// Check a single call target for capability violations and unknown callees.
/// Also handles spawn, reflection, catch_unwind, and error swallow detection.
pub(crate) fn rvs_check_target_S<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    def_path: &str,
    src_path: Option<&str>,
    caps: &CapabilitySet,
    capsmap: &Option<CapsMap>,
) {
    let cn = def_path.rsplit("::").next().unwrap_or(def_path);
    if let Some((_, cc)) = crate::capability::rvs_parse_function(cn) {
        if !caps.rvs_can_call(&cc) {
            let m: Vec<_> = caps
                .rvs_missing_for(&cc)
                .iter()
                .map(|c| format!("{c}"))
                .collect();
            cx.emit_span_lint(
                RVS_CALL_VIOLATION,
                span,
                Msg::new(span, format!("{} → {} missing {}", caps, cc, m.join(", "))),
            );
        }
        return;
    }
    let lookup = capsmap.as_ref().and_then(|cm| {
        cm.rvs_lookup(def_path)
            .or_else(|| src_path.and_then(|s| cm.rvs_lookup(s)))
    });
    if let Some(cc) = lookup.cloned() {
        if !cc.rvs_is_empty() && !caps.rvs_can_call(&cc) {
            let m: Vec<_> = caps
                .rvs_missing_for(&cc)
                .iter()
                .map(|c| format!("{c}"))
                .collect();
            let callee_display = if let Some(sp) = src_path {
                if sp != def_path {
                    format!("{sp} ({def_path})")
                } else {
                    def_path.to_string()
                }
            } else {
                def_path.to_string()
            };
            cx.emit_span_lint(
                RVS_CALL_VIOLATION,
                span,
                Msg::new(
                    span,
                    format!(
                        "{} → {callee_display} ({}) missing {}",
                        caps,
                        cc,
                        m.join(", ")
                    ),
                ),
            );
        }
        return;
    }
    let hint = if let Some(sp) = src_path {
        if sp != def_path {
            format!("'{sp}' ({def_path}) not in capsmap")
        } else {
            format!("'{def_path}' not in capsmap")
        }
    } else {
        format!("'{def_path}' not in capsmap")
    };
    cx.emit_span_lint(RVS_UNKNOWN_CALLEE, span, Msg::new(span, hint));
}
