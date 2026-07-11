use rustc_lint::{LateContext, LintContext};

use super::super::RVS_VALIDATE_RETURNS_UNIT;
use super::super::msg::Msg;
use super::super::utils::VALIDATE_PREFIXES;
use super::result_return::rvs_result_return;

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

    if rvs_result_return(sig).is_some_and(|result| result.rvs_ok_is_unit()) {
        cx.emit_span_lint(
            RVS_VALIDATE_RETURNS_UNIT,
            sig.span,
            Msg::rvs_new(
                sig.span,
                format!(
                    "{name}: validate returning Result<(),E> — use TryFrom returning Result<T,E>"
                ),
            ),
        );
    }
}
