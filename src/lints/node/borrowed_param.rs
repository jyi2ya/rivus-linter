use rustc_hir::{self, FieldDef, Mutability, TyKind};
use rustc_lint::{LateContext, LintContext};

use super::super::RVS_BORROWED_PARAM;
use super::super::msg::Msg;
use super::super::utils::{BORROWED_TYPES, rvs_ty_last_ident};

fn rvs_borrowed_type(ty: &rustc_hir::Ty<'_>) -> Option<(String, &'static str)> {
    let TyKind::Ref(_, mt) = &ty.kind else {
        return None;
    };
    if mt.mutbl != Mutability::Not {
        return None;
    }

    let name = rvs_ty_last_ident(mt.ty)?;
    if !BORROWED_TYPES.contains(&name.as_str()) {
        return None;
    }

    let better = match name.as_str() {
        "String" => "&str",
        "Vec" => "&[T]",
        "Box" => "&T",
        _ => return None,
    };
    Some((name, better))
}

/// Check function parameters for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_fn_params_S<'tcx>(cx: &LateContext<'_>, sig: &rustc_hir::FnSig<'tcx>) {
    for input in sig.decl.inputs {
        if let Some((name, better)) = rvs_borrowed_type(input) {
            cx.emit_span_lint(
                RVS_BORROWED_PARAM,
                input.span,
                Msg::rvs_new(input.span, format!("&{name} — use {better} instead")),
            );
        }
    }
}

/// Check struct fields for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_borrowed_fields_S<'tcx>(cx: &LateContext<'_>, fields: &[FieldDef<'tcx>]) {
    for f in fields {
        if let Some((name, better)) = rvs_borrowed_type(f.ty) {
            cx.emit_span_lint(
                RVS_BORROWED_PARAM,
                f.ty.span,
                Msg::rvs_new(f.ty.span, format!("&{name} field — use {better} instead")),
            );
        }
    }
}
