use rustc_hir::{self, FnRetTy, QPath, TyKind};
use rustc_lint::{LateContext, LintContext};

use super::RVS_VALIDATE_RETURNS_UNIT;
use super::msg::Msg;
use super::utils::{VALIDATE_PREFIXES, rvs_generic_args_result_type, rvs_plast, rvs_tys};

/// Check for validate/check/verify functions returning `Result<(), E>` —
/// should use TryFrom instead.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'_>, name: &str, sig: &rustc_hir::FnSig<'tcx>) {
    let base = name
        .strip_prefix("rvs_")
        .unwrap_or(name)
        .split('_')
        .next()
        .unwrap_or("");
    let lower = base.to_ascii_lowercase();
    if !VALIDATE_PREFIXES.iter().any(|p| lower == *p) {
        return;
    }

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

    if type_args.len() >= 1 {
        let ok_str = rvs_tys(type_args[0]);
        if ok_str == "()" {
            cx.emit_span_lint(
                RVS_VALIDATE_RETURNS_UNIT,
                sig.span,
                Msg::new(
                    sig.span,
                    format!(
                        "{name}: validate returning Result<(),E> — use TryFrom returning Result<T,E>"
                    ),
                ),
            );
        }
    }
}
