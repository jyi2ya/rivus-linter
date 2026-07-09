use rustc_hir::{self, Body, ExprKind};
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::port_traits;
use super::utils::{rvs_def_path, rvs_qp, rvs_walk_closures};
use super::{RVS_CALL_VIOLATION, RVS_UNKNOWN_CALLEE};
use crate::capability::{Capability, CapabilitySet};
use crate::capsmap::CapsMap;
use crate::inference::{
    CallContractMismatch, CallContractMismatchKind, rvs_collect_call_contract_mismatch,
    rvs_collect_named_call_contract_mismatch,
};
use crate::symbols::DefPath;
use std::collections::HashSet;

fn rvs_lookup_caps_exact<'a>(
    capsmap: &'a Option<CapsMap>,
    def_path: &str,
) -> Option<&'a CapabilitySet> {
    capsmap.as_ref().and_then(|cm| cm.rvs_lookup(def_path))
}

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
                        rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn
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
        if let Some(mismatch) =
            rvs_collect_call_contract_mismatch(def_path, src_path, caps, Some(&cc))
        {
            rvs_emit_call_contract_mismatch_S(cx, span, caps, &mismatch);
        }
        return;
    }

    let cn = DefPath::from(def_path).rvs_fn_name();
    let raw_suffix = crate::capability::rvs_extract_raw_suffix(cn.rvs_as_str());
    let has_unknown_suffix = raw_suffix
        .chars()
        .any(|letter| crate::capability::Capability::rvs_from_char(letter).is_none());
    if let Some((_, cc)) = crate::capability::rvs_parse_function(cn.rvs_as_str()) {
        if !has_unknown_suffix || !cc.rvs_is_empty() {
            if let Some(mismatch) =
                rvs_collect_named_call_contract_mismatch(def_path, src_path, caps, &cc)
            {
                rvs_emit_call_contract_mismatch_S(cx, span, caps, &mismatch);
            }
        } else if def_id.as_local().is_none() {
            // External unknown-only suffixes still need a capsmap entry. Local
            // declarations are diagnosed by the suffix lint at the declaration.
            let lookup = rvs_lookup_caps_exact(capsmap, def_path);
            if let Some(mismatch) =
                rvs_collect_call_contract_mismatch(def_path, src_path, caps, lookup)
            {
                rvs_emit_call_contract_mismatch_S(cx, span, caps, &mismatch);
            }
        }
        return;
    }
    let lookup = rvs_lookup_caps_exact(capsmap, def_path);
    if let Some(mismatch) = rvs_collect_call_contract_mismatch(def_path, src_path, caps, lookup) {
        rvs_emit_call_contract_mismatch_S(cx, span, caps, &mismatch);
    }
}

fn rvs_emit_call_contract_mismatch_S<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    caps: &CapabilitySet,
    mismatch: &CallContractMismatch,
) {
    match mismatch.kind {
        CallContractMismatchKind::UnknownCallee => {
            let hint = rvs_unknown_callee_hint(&mismatch.callee_display);
            cx.emit_span_lint(RVS_UNKNOWN_CALLEE, span, Msg::rvs_new(span, hint));
        }
        CallContractMismatchKind::MissingCapabilities => {
            let callee_caps = mismatch
                .callee_caps
                .as_ref()
                .expect("never: missing-capability mismatch must carry callee caps");
            let missing: Vec<_> = mismatch
                .missing_caps
                .iter()
                .map(|c| format!("{c}"))
                .collect();
            let message = if mismatch.callee_display.is_empty() {
                format!("{} → {} missing {}", caps, callee_caps, missing.join(", "))
            } else {
                format!(
                    "{} → {} ({}) missing {}",
                    caps,
                    mismatch.callee_display,
                    callee_caps,
                    missing.join(", ")
                )
            };
            cx.emit_span_lint(RVS_CALL_VIOLATION, span, Msg::rvs_new(span, message));
        }
    }
}

fn rvs_unknown_callee_hint(callee_display: &str) -> String {
    if let Some((src_path, def_path)) = callee_display.rsplit_once(" (") {
        format!("'{src_path}' ({def_path} not in capsmap")
    } else {
        format!("'{callee_display}' not in capsmap")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::rvs_make_callee_display;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260703_lookup_caps_requires_exact_def_path() {
        let capsmap = Some(CapsMap::rvs_parse("read=BI\nstd::fs::read_to_string=BI\n").unwrap());

        let exact = rvs_lookup_caps_exact(&capsmap, "std::fs::read_to_string").is_some();
        let short = rvs_lookup_caps_exact(&capsmap, "std::fs::read").is_some();
        rvs_snapshot_BIS(
            "test_20260703_lookup_caps_requires_exact_def_path",
            &format!("exact={exact}\nshort={short}\n"),
        );

        assert!(exact);
        assert!(!short);
    }

    #[test]
    fn test_20260703_collect_call_contract_mismatch() {
        let caller_caps = CapabilitySet::rvs_from_validated("A");
        let callee_caps = CapabilitySet::rvs_from_validated("AP");
        let mismatch = rvs_collect_call_contract_mismatch(
            "demo::rvs_fetch_P",
            Some("rvs_fetch_P"),
            &caller_caps,
            Some(&callee_caps),
        )
        .expect("expected call mismatch");
        rvs_snapshot_BIS(
            "test_20260703_collect_call_contract_mismatch",
            &format!("mismatch={mismatch:?}\n"),
        );

        assert_eq!(mismatch.kind, CallContractMismatchKind::MissingCapabilities);
        assert!(mismatch.missing_caps.contains(&Capability::P));
    }

    #[test]
    fn test_20260703_collect_named_call_contract_mismatch() {
        let caller_caps = CapabilitySet::rvs_new();
        let callee_caps = CapabilitySet::rvs_from_validated("BI");
        let mismatch = rvs_collect_named_call_contract_mismatch(
            "demo::rvs_fetch_BI",
            Some("rvs_fetch_BI"),
            &caller_caps,
            &callee_caps,
        )
        .expect("expected named call mismatch");
        rvs_snapshot_BIS(
            "test_20260703_collect_named_call_contract_mismatch",
            &format!("mismatch={mismatch:?}\n"),
        );

        assert_eq!(mismatch.callee_display, "rvs_fetch_BI (demo::rvs_fetch_BI)");
        assert_eq!(mismatch.kind, CallContractMismatchKind::MissingCapabilities);
    }

    #[test]
    fn test_20260704_trait_impl_def_path_uses_method_name_before_trait_suffix() {
        let fn_name = DefPath::from("demo::Adapter::rvs_fetch_BI@demo::ApiClient").rvs_fn_name();
        let parsed = crate::capability::rvs_parse_function(fn_name.rvs_as_str())
            .map(|(_, caps)| caps)
            .expect("trait impl method name should parse");
        let output = format!("fn_name={fn_name}\ncaps={parsed}\n");
        rvs_snapshot_BIS(
            "test_20260704_trait_impl_def_path_uses_method_name_before_trait_suffix",
            &output,
        );

        assert_eq!(fn_name.rvs_as_str(), "rvs_fetch_BI");
        assert!(parsed.rvs_contains(Capability::B));
        assert!(parsed.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260703_make_callee_display() {
        let output = format!(
            "qualified={}\nplain={}\n",
            rvs_make_callee_display("demo::rvs_fetch_P", Some("rvs_fetch_P")),
            rvs_make_callee_display("demo::rvs_fetch_P", Some("demo::rvs_fetch_P")),
        );
        rvs_snapshot_BIS("test_20260703_make_callee_display", &output);

        assert_eq!(
            rvs_make_callee_display("demo::rvs_fetch_P", Some("rvs_fetch_P")),
            "rvs_fetch_P (demo::rvs_fetch_P)"
        );
        assert_eq!(
            rvs_make_callee_display("demo::rvs_fetch_P", Some("demo::rvs_fetch_P")),
            "demo::rvs_fetch_P"
        );
    }

    #[test]
    fn test_20260705_unknown_callee_hint_quotes_source_path_only() {
        let hint = rvs_unknown_callee_hint("rvs_fetch_E (demo::rvs_fetch_E)");
        rvs_snapshot_BIS(
            "test_20260705_unknown_callee_hint_quotes_source_path_only",
            &hint,
        );
        assert_eq!(hint, "'rvs_fetch_E' (demo::rvs_fetch_E) not in capsmap");
    }
}
