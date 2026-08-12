use rustc_lint::LateContext;

use super::super::RVS_VALIDATE_RETURNS_UNIT;
use super::super::msg::rvs_emit_span_lint_S;
use super::super::utils::VALIDATE_PREFIXES;
use super::result_return::rvs_result_return;
use crate::capability::CapabilitySet;

/// Check for pure validate/check/verify functions returning `Result<(), E>` —
/// should use TryFrom instead.
///
/// Only fires on functions whose effective capability set is empty (pure).
/// Functions with any capability (A, B, I, M, P, S, T, U) perform effects or
/// mutate state, so `TryFrom` — a pure value transformation — does not apply.
///
/// `effective_caps` includes structurally inferred capabilities such as
/// World Port `P`, which are not visible in the function name suffix alone.
pub(crate) fn rvs_check_fn_S<'tcx>(
    cx: &LateContext<'tcx>,
    name: &str,
    sig: &'tcx rustc_hir::FnSig<'tcx>,
    effective_caps: &CapabilitySet,
) {
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

    if !effective_caps.rvs_is_empty() {
        return;
    }

    if rvs_result_return(cx, sig).is_some_and(|result| result.rvs_ok_is_unit()) {
        rvs_emit_span_lint_S(
            cx,
            RVS_VALIDATE_RETURNS_UNIT,
            sig.span,
            format!("{name}: validate returning Result<(),E> — use TryFrom returning Result<T,E>"),
        );
    }
}
