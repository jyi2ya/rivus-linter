use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::port_traits;
use super::utils::{rvs_def_path, rvs_qp, rvs_walk_closures};
use super::{RVS_CALL_VIOLATION, RVS_UNKNOWN_CALLEE};
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet};
use crate::capsmap::CapsMap;
use std::collections::HashSet;

/// Walk the function body checking all call targets for capability violations
/// and unknown callees.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    caps: &CapabilitySet,
    capsmap: &Option<CapsMap>,
    port_traits: &HashSet<rustc_span::def_id::DefId>,
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
                        rvs_check_target_S(
                            cx,
                            e.span,
                            did,
                            &fp,
                            Some(&sp),
                            caps,
                            capsmap,
                            port_traits,
                        );
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
                rvs_check_target_S(cx, e.span, did, &fp, Some(n), caps, capsmap, port_traits);
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
    def_id: rustc_span::def_id::DefId,
    def_path: &str,
    src_path: Option<&str>,
    caps: &CapabilitySet,
    capsmap: &Option<CapsMap>,
    port_traits: &HashSet<rustc_span::def_id::DefId>,
) {
    if port_traits::rvs_is_port_method_def_id(cx, def_id, port_traits) {
        let mut cc = CapabilitySet::rvs_new();
        cc.rvs_insert_M(Capability::P);
        rvs_emit_call_violation_if_needed_S(cx, span, def_path, src_path, caps, &cc);
        return;
    }

    let cn = def_path.rsplit("::").next().unwrap_or(def_path);
    if let Some((_, cc)) = crate::capability::rvs_parse_function(cn) {
        rvs_emit_named_call_violation_if_needed_S(cx, span, caps, &cc);
        return;
    }
    let lookup = capsmap.as_ref().and_then(|cm| {
        cm.rvs_lookup(def_path)
            .or_else(|| src_path.and_then(|s| cm.rvs_lookup(s)))
    });
    if let Some(cc) = lookup.cloned() {
        rvs_emit_call_violation_if_needed_S(cx, span, def_path, src_path, caps, &cc);
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

fn rvs_emit_call_violation_if_needed_S<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    def_path: &str,
    src_path: Option<&str>,
    caps: &CapabilitySet,
    callee_caps: &CapabilitySet,
) {
    if callee_caps.rvs_is_empty() || CapabilityPolicy::rvs_can_call(caps, callee_caps) {
        return;
    }
    let missing: Vec<_> = CapabilityPolicy::rvs_missing_for(caps, callee_caps)
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
                callee_caps,
                missing.join(", ")
            ),
        ),
    );
}

fn rvs_emit_named_call_violation_if_needed_S<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    caps: &CapabilitySet,
    callee_caps: &CapabilitySet,
) {
    if callee_caps.rvs_is_empty() || CapabilityPolicy::rvs_can_call(caps, callee_caps) {
        return;
    }
    let missing: Vec<_> = CapabilityPolicy::rvs_missing_for(caps, callee_caps)
        .iter()
        .map(|c| format!("{c}"))
        .collect();
    cx.emit_span_lint(
        RVS_CALL_VIOLATION,
        span,
        Msg::new(
            span,
            format!("{} → {} missing {}", caps, callee_caps, missing.join(", ")),
        ),
    );
}
