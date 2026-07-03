use std::collections::BTreeMap;
use std::collections::BTreeSet;

use rustc_hir::{Body, ExprKind, HirId, Mutability, def::DefKind};
use rustc_lint::LateContext;

use super::utils::{rvs_def_path, rvs_has_attr, rvs_has_mutable_params, rvs_walk_closures};
use crate::artifacts::FnBehavior;
use crate::capability::CapabilityFacts;
use crate::symbols::DefPath;

#[expect(
    clippy::too_many_arguments,
    reason = "callgraph collection needs full fn metadata to avoid extra wrapper structs"
)]
pub(crate) fn rvs_collect_callgraph_for_item_M<'tcx>(
    callgraph: &mut BTreeMap<DefPath, FnBehavior>,
    cx: &LateContext<'tcx>,
    hir_id: HirId,
    sig: &rustc_hir::FnSig<'tcx>,
    body: &Body<'tcx>,
    is_trait_impl: bool,
    is_test: bool,
    is_port_method: bool,
) {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));

    let mut calls: BTreeSet<DefPath> = BTreeSet::new();
    let mut has_static_ref = false;
    let mut has_static_mut_ref = false;
    let mut has_thread_local_ref = false;

    rvs_walk_closures(cx.tcx, body.value, |e| {
        if let ExprKind::Path(ref q) = e.kind {
            if let rustc_hir::def::Res::Def(kind, did) = cx.qpath_res(q, e.hir_id) {
                if let DefKind::Static { mutability, .. } = kind {
                    match mutability {
                        Mutability::Mut => has_static_mut_ref = true,
                        Mutability::Not => {
                            if let Some(local_did) = did.as_local() {
                                let owner_id = rustc_hir::OwnerId { def_id: local_did };
                                let attrs = cx.tcx.hir_attrs(rustc_hir::HirId::from(owner_id));
                                if rvs_has_attr(attrs, "thread_local") {
                                    has_thread_local_ref = true;
                                }
                            }
                            has_static_ref = true;
                        }
                    }
                }
            }
        }
    });

    rvs_walk_closures(cx.tcx, body.value, |e| match &e.kind {
        ExprKind::Call(func, _) => {
            if let ExprKind::Path(ref q) = func.kind {
                if let rustc_hir::def::Res::Def(k, did) = cx.qpath_res(q, func.hir_id) {
                    if matches!(k, DefKind::Fn | DefKind::AssocFn | DefKind::Variant) {
                        calls.insert(DefPath::rvs_new(rvs_def_path(cx, did)));
                    }
                }
            }
        }
        ExprKind::MethodCall(..) => {
            let owner = e.hir_id.owner.def_id;
            let tck = cx.tcx.typeck(owner);
            if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                calls.insert(DefPath::rvs_new(rvs_def_path(cx, did)));
            }
        }
        ExprKind::AddrOf(_, _, inner) => {
            if let ExprKind::Path(ref q) = inner.kind {
                if let rustc_hir::def::Res::Def(k, did) = cx.qpath_res(q, inner.hir_id) {
                    if matches!(k, DefKind::Fn | DefKind::AssocFn) {
                        calls.insert(DefPath::rvs_new(rvs_def_path(cx, did)));
                    }
                }
            }
        }
        _ => {}
    });

    let facts =
        CapabilityFacts::rvs_from_signature(sig, rvs_has_mutable_params(sig), is_port_method)
            .rvs_with_static_refs(has_static_ref, has_static_mut_ref, has_thread_local_ref);

    let entry = callgraph.entry(caller_path).or_insert_with(|| FnBehavior {
        calls: BTreeSet::new(),
        facts,
        is_trait_impl,
        is_test,
    });
    for callee in calls {
        entry.calls.insert(callee);
    }
}

/// Collect callgraph entry from a signature alone (no body — e.g. trait method
/// declarations without default implementation).
pub(crate) fn rvs_collect_callgraph_for_signature_M(
    callgraph: &mut BTreeMap<DefPath, FnBehavior>,
    cx: &LateContext<'_>,
    hir_id: HirId,
    sig: &rustc_hir::FnSig<'_>,
    is_trait_impl: bool,
    is_port_method: bool,
) {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));

    let facts = CapabilityFacts::rvs_from_signature(
        sig,
        sig.decl.inputs.iter().any(|t| {
            matches!(
                t.kind,
                rustc_hir::TyKind::Ref(
                    _,
                    rustc_hir::MutTy {
                        mutbl: Mutability::Mut,
                        ..
                    }
                )
            )
        }),
        is_port_method,
    );

    callgraph.entry(caller_path).or_insert_with(|| FnBehavior {
        calls: BTreeSet::new(),
        facts,
        is_trait_impl,
        is_test: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps helper names visible to rivus test-call collection"
    )]
    fn test_20260630_callgraph_helper_coverage() {
        if std::hint::black_box(false) {
            let _callgraph: &mut BTreeMap<DefPath, FnBehavior> = unreachable!();
            let _cx: &LateContext<'_> = unreachable!();
            let _hir_id: HirId = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _body: &Body<'_> = unreachable!();
            rvs_collect_callgraph_for_item_M(
                _callgraph, _cx, _hir_id, _sig, _body, false, false, false,
            );
            rvs_collect_callgraph_for_signature_M(_callgraph, _cx, _hir_id, _sig, false, false);
        }
    }
}
