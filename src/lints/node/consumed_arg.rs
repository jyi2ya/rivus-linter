use std::collections::HashSet;

use rustc_hir::{self, TyKind};
use rustc_lint::{LateContext, LintContext};

use super::super::RVS_CONSUMED_ARG_ON_ERROR;
use super::super::msg::Msg;
use super::super::utils::{rvs_collect_type_idents_M, rvs_plast};
use super::result_return::rvs_result_return;

/// Check that owned (non-ref) parameters are preserved in the error type when
/// the function returns `Result<(), E>`.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'_>,
    sig: &rustc_hir::FnSig<'tcx>,
    fn_name: &str,
) {
    let Some(result) = rvs_result_return(sig) else {
        return;
    };
    if !result.rvs_ok_is_unit() {
        return;
    }

    let mut error_idents = HashSet::new();
    rvs_collect_type_idents_M(result.error, &mut error_idents);

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
