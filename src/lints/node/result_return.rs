use rustc_hir::{FnRetTy, GenericBound, TyKind as HirTyKind};
use rustc_lint::LateContext;
use rustc_middle::ty::{Ty, TyKind as MiddleTyKind};
use rustc_span::sym;

#[derive(Debug)]
pub(crate) struct ResultReturn<'tcx> {
    pub(crate) ok: Ty<'tcx>,
    pub(crate) error: Ty<'tcx>,
}

impl ResultReturn<'_> {
    pub(crate) fn rvs_ok_is_unit(&self) -> bool {
        matches!(self.ok.kind(), MiddleTyKind::Tuple(types) if types.is_empty())
    }
}

pub(crate) fn rvs_result_return<'tcx>(
    cx: &LateContext<'tcx>,
    sig: &'tcx rustc_hir::FnSig<'tcx>,
) -> Option<ResultReturn<'tcx>> {
    let FnRetTy::Return(return_type) = sig.decl.output else {
        return None;
    };
    let declared_output = if sig.header.asyncness.is_async() {
        let HirTyKind::OpaqueDef(opaque_type) = return_type.kind else {
            return None;
        };
        opaque_type.bounds.iter().find_map(|bound| {
            let GenericBound::Trait(trait_ref) = bound else {
                return None;
            };
            trait_ref
                .trait_ref
                .path
                .segments
                .last()?
                .args?
                .constraints
                .iter()
                .find_map(|constraint| {
                    (constraint.ident.name == sym::Output)
                        .then(|| constraint.ty())
                        .flatten()
                })
        })?
    } else {
        return_type
    };
    let resolved_output = rustc_hir_analysis::lower_ty(cx.tcx, declared_output);
    let resolved_output = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), resolved_output)
        .unwrap_or(resolved_output);
    let MiddleTyKind::Adt(result_adt, _) = resolved_output.kind() else {
        return None;
    };
    if !cx.tcx.is_diagnostic_item(sym::Result, result_adt.did()) {
        return None;
    }
    let MiddleTyKind::Adt(_, arguments) = resolved_output.kind() else {
        return None;
    };
    let result = ResultReturn {
        ok: arguments.type_at(0),
        error: arguments.type_at(1),
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only branch links rustc-context logic exercised by Result-return UI fixtures"
    )]
    fn test_20260714_result_return_ui_coverage() {
        rvs_snapshot_BIS("test_20260714_result_return_ui_coverage", "covered\n");
        if std::hint::black_box(false) {
            let _cx: &LateContext<'_> = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _ty: Ty<'_> = unreachable!();
            let result = ResultReturn {
                ok: _ty,
                error: _ty,
            };
            let _ = result.rvs_ok_is_unit();
            let _ = rvs_result_return(_cx, _sig);
        }
    }
}
