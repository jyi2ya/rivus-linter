use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use rustc_hir::{Body, ExprKind, HirId, Mutability, def::DefKind};
use rustc_lint::LateContext;

use super::utils::*;

#[derive(Debug, Serialize)]
pub(crate) struct FnReportEntry {
    pub name: String,
    pub caps: String,
    pub lines: usize,
    pub is_test: bool,
    pub allows_dead_code: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct FnBehavior {
    pub calls: BTreeSet<String>,
    pub has_async: bool,
    pub is_unsafe_fn: bool,
    pub has_mut_param: bool,
    pub has_static_ref: bool,
    pub has_static_mut_ref: bool,
    pub has_thread_local_ref: bool,
    pub is_trait_impl: bool,
    pub is_test: bool,
    #[serde(default)]
    pub is_port_method: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rvs_collect_callgraph_for_item_M<'tcx>(
    callgraph: &mut BTreeMap<String, FnBehavior>,
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
    let caller_path = rvs_def_path(cx, def_id);

    let mut calls: BTreeSet<String> = BTreeSet::new();
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
                        calls.insert(rvs_def_path(cx, did));
                    }
                }
            }
        }
        ExprKind::MethodCall(..) => {
            let owner = e.hir_id.owner.def_id;
            let tck = cx.tcx.typeck(owner);
            if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                calls.insert(rvs_def_path(cx, did));
            }
        }
        ExprKind::AddrOf(_, _, inner) => {
            if let ExprKind::Path(ref q) = inner.kind {
                if let rustc_hir::def::Res::Def(k, did) = cx.qpath_res(q, inner.hir_id) {
                    if matches!(k, DefKind::Fn | DefKind::AssocFn) {
                        calls.insert(rvs_def_path(cx, did));
                    }
                }
            }
        }
        _ => {}
    });

    let has_async = sig.header.asyncness.is_async();
    let is_unsafe_fn = matches!(
        sig.header.safety,
        rustc_hir::HeaderSafety::Normal(rustc_hir::Safety::Unsafe)
    );
    let has_mut_param = rvs_has_mutable_params(sig, body);

    let entry = callgraph.entry(caller_path).or_insert_with(|| FnBehavior {
        calls: BTreeSet::new(),
        has_async,
        is_unsafe_fn,
        has_mut_param,
        has_static_ref,
        has_static_mut_ref,
        has_thread_local_ref,
        is_trait_impl,
        is_test,
        is_port_method,
    });
    for callee in calls {
        entry.calls.insert(callee);
    }
}

/// Collect callgraph entry from a signature alone (no body — e.g. trait method
/// declarations without default implementation).
pub(crate) fn rvs_collect_callgraph_for_signature_M(
    callgraph: &mut BTreeMap<String, FnBehavior>,
    cx: &LateContext<'_>,
    hir_id: HirId,
    sig: &rustc_hir::FnSig<'_>,
    is_trait_impl: bool,
    is_port_method: bool,
) {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = rvs_def_path(cx, def_id);

    let has_async = sig.header.asyncness.is_async();
    let is_unsafe_fn = matches!(
        sig.header.safety,
        rustc_hir::HeaderSafety::Normal(rustc_hir::Safety::Unsafe)
    );
    let has_mut_param = sig.decl.inputs.iter().any(|t| {
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
    });

    callgraph.entry(caller_path).or_insert_with(|| FnBehavior {
        calls: BTreeSet::new(),
        has_async,
        is_unsafe_fn,
        has_mut_param,
        has_static_ref: false,
        has_static_mut_ref: false,
        has_thread_local_ref: false,
        is_trait_impl,
        is_test: false,
        is_port_method,
    });
}
