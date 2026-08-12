use rustc_hir::{self, FieldDef, Mutability};
use rustc_lint::LateContext;
use rustc_middle::ty::TyKind;
use rustc_span::sym;

use super::super::RVS_BORROWED_PARAM;
use super::super::msg::rvs_emit_node_span_lint_S;

fn rvs_borrowed_type<'tcx>(
    cx: &LateContext<'tcx>,
    ty: &'tcx rustc_hir::Ty<'tcx>,
) -> Option<(&'static str, &'static str)> {
    let resolved = rustc_hir_analysis::lower_ty(cx.tcx, ty);
    let resolved = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), resolved)
        .unwrap_or(resolved);
    let TyKind::Ref(_, borrowed, mutability) = resolved.kind() else {
        return None;
    };
    if *mutability != Mutability::Not {
        return None;
    }
    let TyKind::Adt(adt, _) = borrowed.kind() else {
        return None;
    };
    let def_id = adt.did();

    let (name, better) = if cx.tcx.is_lang_item(def_id, rustc_hir::LangItem::String) {
        ("String", "&str")
    } else if cx.tcx.is_diagnostic_item(sym::Vec, def_id) {
        ("Vec", "&[T]")
    } else if cx.tcx.lang_items().owned_box() == Some(def_id) {
        ("Box", "&T")
    } else {
        return None;
    };
    Some((name, better))
}

/// Check function parameters for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_fn_params_S<'tcx>(
    cx: &LateContext<'tcx>,
    sig: &'tcx rustc_hir::FnSig<'tcx>,
    params: &'tcx [rustc_hir::Param<'tcx>],
) {
    debug_assert_eq!(sig.decl.inputs.len(), params.len());
    for (input, param) in sig.decl.inputs.iter().zip(params) {
        if let Some((name, better)) = rvs_borrowed_type(cx, input) {
            rvs_emit_node_span_lint_S(
                cx,
                RVS_BORROWED_PARAM,
                param.hir_id,
                input.span,
                format!("&{name} — use {better} instead"),
            );
        }
    }
}

/// Check struct fields for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_borrowed_fields_S<'tcx>(cx: &LateContext<'tcx>, fields: &[FieldDef<'tcx>]) {
    for f in fields {
        if let Some((name, better)) = rvs_borrowed_type(cx, f.ty) {
            rvs_emit_node_span_lint_S(
                cx,
                RVS_BORROWED_PARAM,
                f.hir_id,
                f.ty.span,
                format!("&{name} field — use {better} instead"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};

    #[test]
    fn test_20260714_borrowed_type_ui_coverage() {
        rvs_snapshot_BIS("test_20260714_borrowed_type_ui_coverage", "covered\n");
        rvs_register_test_coverage(rvs_borrowed_type);
    }
}
