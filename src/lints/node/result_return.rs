use rustc_hir::{FnRetTy, GenericArg, QPath, Ty, TyKind};

#[derive(Debug)]
pub(crate) struct ResultReturn<'tcx> {
    pub(crate) ok: &'tcx Ty<'tcx>,
    pub(crate) error: &'tcx Ty<'tcx>,
}

impl ResultReturn<'_> {
    pub(crate) fn rvs_ok_is_unit(&self) -> bool {
        matches!(self.ok.kind, TyKind::Tup(types) if types.is_empty())
    }
}

pub(crate) fn rvs_result_return<'tcx>(
    sig: &'tcx rustc_hir::FnSig<'tcx>,
) -> Option<ResultReturn<'tcx>> {
    let FnRetTy::Return(return_type) = sig.decl.output else {
        return None;
    };
    let TyKind::Path(QPath::Resolved(_, path)) = &return_type.kind else {
        return None;
    };
    let result_segment = path.segments.last()?;
    if result_segment.ident.name.as_str() != "Result" {
        return None;
    }
    let mut types = result_segment.args?.args.iter().filter_map(|argument| {
        if let GenericArg::Type(ty) = argument {
            Some(ty.as_unambig_ty())
        } else {
            None
        }
    });
    let result = ResultReturn {
        ok: types.next()?,
        error: types.next()?,
    };
    let extra_type = types.next();
    debug_assert!(
        extra_type.is_none(),
        "Result has exactly two type arguments"
    );
    Some(result)
}
