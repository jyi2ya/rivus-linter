use rustc_hir;
use rustc_lint::LateContext;
use rustc_middle::ty::{TyKind, TypeVisitableExt};

use super::super::RVS_CONSUMED_ARG_ON_ERROR;
use super::super::msg::Msg;
use super::super::utils::rvs_tys;
use super::result_return::rvs_result_return;

/// Check that owned (non-ref) parameters are preserved in the error type when
/// the function returns `Result<(), E>`.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    sig: &'tcx rustc_hir::FnSig<'tcx>,
    params: &'tcx [rustc_hir::Param<'tcx>],
    fn_name: &str,
) {
    let Some(result) = rvs_result_return(cx, sig) else {
        return;
    };
    if !result.rvs_ok_is_unit() {
        return;
    }

    let Some(first_input) = sig.decl.inputs.first() else {
        return;
    };
    let resolved_inputs = cx
        .tcx
        .fn_sig(first_input.hir_id.owner.def_id.to_def_id())
        .skip_binder()
        .inputs()
        .skip_binder();
    debug_assert_eq!(sig.decl.inputs.len(), params.len());
    for ((input, resolved_type), param) in sig.decl.inputs.iter().zip(resolved_inputs).zip(params) {
        let resolved_type = cx
            .tcx
            .try_normalize_erasing_regions(cx.typing_env(), *resolved_type)
            .unwrap_or(*resolved_type);
        if matches!(resolved_type.kind(), TyKind::Ref(..)) {
            continue;
        }
        if resolved_type.has_escaping_bound_vars() {
            continue;
        }
        if cx.type_is_copy_modulo_regions(resolved_type) {
            continue;
        }
        if !result.error.contains(resolved_type) {
            let param_type = rvs_tys(input);
            cx.tcx.emit_node_span_lint(
                RVS_CONSUMED_ARG_ON_ERROR,
                param.hir_id,
                input.span,
                Msg::rvs_new(
                    input.span,
                    format!(
                        "owned param '{param_type}' consumed but not preserved in error type of {fn_name}"
                    ),
                ),
            );
        }
    }
}
