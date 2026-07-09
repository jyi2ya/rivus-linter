use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_NON_RVS_FN;
use super::msg::Msg;
use crate::inference::{FnContractMismatch, FnContractMismatchKind};

/// Check for functions missing the `rvs_` prefix.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, name: &str, span: Span) {
    if !name.starts_with("rvs_") {
        cx.emit_span_lint(
            RVS_NON_RVS_FN,
            span,
            Msg::rvs_new(span, format!("'{name}' missing rvs_ prefix")),
        );
    }
}

pub(crate) fn rvs_check_contract_mismatches_S(
    cx: &LateContext<'_>,
    name: &str,
    mismatches: &[FnContractMismatch],
    span: Span,
    is_test: bool,
    is_trait_impl_method: bool,
) {
    if is_test || is_trait_impl_method {
        return;
    }
    if mismatches
        .iter()
        .any(|mismatch| mismatch.kind == FnContractMismatchKind::MissingRvsPrefix)
    {
        cx.emit_span_lint(
            RVS_NON_RVS_FN,
            span,
            Msg::rvs_new(span, format!("'{name}' missing rvs_ prefix")),
        );
    }
}
