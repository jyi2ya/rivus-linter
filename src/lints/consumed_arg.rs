use std::collections::HashSet;

use rustc_hir::{self, FnRetTy, QPath, TyKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_CONSUMED_ARG_ON_ERROR;
use super::msg::Msg;
use super::utils::{rvs_collect_type_idents_M, rvs_generic_args_result_type, rvs_plast, rvs_tys};

/// Check that owned (non-ref) parameters are preserved in the error type when
/// the function returns `Result<(), E>`.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'_>,
    sig: &rustc_hir::FnSig<'tcx>,
    fn_name: &str,
) {
    let FnRetTy::Return(ret_ty) = sig.decl.output else {
        return;
    };
    let TyKind::Path(ref q) = ret_ty.kind else {
        return;
    };
    let result_name = rvs_plast(q);
    if result_name.as_deref() != Some("Result") {
        return;
    }

    let type_args = match q {
        QPath::Resolved(_, p) => p
            .segments
            .first()
            .and_then(|s| s.args)
            .map(|ga| rvs_generic_args_result_type(Some(ga)))
            .unwrap_or_else(Vec::new),
        _ => return,
    };

    let Some(ok_ty) = type_args.first() else {
        return;
    };
    if rvs_tys(ok_ty) != "()" {
        return;
    }

    let mut error_idents = HashSet::new();
    if let Some(error_ty) = type_args.get(1) {
        rvs_collect_type_idents_M(error_ty, &mut error_idents);
    }

    for input in sig.decl.inputs {
        if let TyKind::Path(ref iq) = input.kind {
            if let Some(param_name) = rvs_plast(iq) {
                let is_ref = matches!(input.kind, TyKind::Ref(_, _));
                if !is_ref && !error_idents.contains(&param_name) {
                    cx.emit_span_lint(
                        RVS_CONSUMED_ARG_ON_ERROR,
                        input.span,
                        Msg::rvs_new(
                            input.span,
                            format!(
                                "owned param '{param_name}' consumed but not preserved in error type of {fn_name}"
                            ),
                        ),
                    );
                }
            }
        }
    }
}
