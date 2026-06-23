use rustc_hir::{self, FieldDef, Mutability, TyKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_BORROWED_PARAM;
use super::msg::Msg;
use super::utils::{BORROWED_TYPES, rvs_ty_last_ident};

/// Check function parameters for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_fn_params_S<'tcx>(cx: &LateContext<'_>, sig: &rustc_hir::FnSig<'tcx>) {
    for input in sig.decl.inputs {
        if let TyKind::Ref(_, mt) = &input.kind {
            if mt.mutbl == Mutability::Not {
                if let Some(name) = rvs_ty_last_ident(mt.ty) {
                    if BORROWED_TYPES.contains(&name.as_str()) {
                        let better = match name.as_str() {
                            "String" => "&str",
                            "Vec" => "&[T]",
                            "Box" => "&T",
                            _ => continue,
                        };
                        cx.emit_span_lint(
                            RVS_BORROWED_PARAM,
                            input.span,
                            Msg::new(input.span, format!("&{name} — use {better} instead")),
                        );
                    }
                }
            }
        }
    }
}

/// Check struct fields for borrowed types (&String/&Vec/&Box).
pub(crate) fn rvs_check_borrowed_fields_S<'tcx>(cx: &LateContext<'_>, fields: &[FieldDef<'tcx>]) {
    for f in fields {
        if let rustc_hir::TyKind::Ref(_, mt) = &f.ty.kind {
            if mt.mutbl == rustc_hir::Mutability::Not {
                if let Some(name) = rvs_ty_last_ident(mt.ty) {
                    if BORROWED_TYPES.contains(&name.as_str()) {
                        let better = match name.as_str() {
                            "String" => "&str",
                            "Vec" => "&[T]",
                            "Box" => "&T",
                            _ => continue,
                        };
                        cx.emit_span_lint(
                            RVS_BORROWED_PARAM,
                            f.ty.span,
                            Msg::new(f.ty.span, format!("&{name} field — use {better} instead")),
                        );
                    }
                }
            }
        }
    }
}
