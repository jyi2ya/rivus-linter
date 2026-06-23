use rustc_hir;
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_DEAD_CODE;
use super::msg::Msg;
use super::utils::rvs_has_allow;

/// Check for `#[allow(dead_code)]` or `#[allow(unused)]` on rvs_ functions.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, attrs: &[rustc_hir::Attribute], span: Span) {
    if rvs_has_allow(attrs, "dead_code") || rvs_has_allow(attrs, "unused") {
        cx.emit_span_lint(
            RVS_DEAD_CODE,
            span,
            Msg::new(span, "rvs_ function marked #[allow(dead_code/unused)]"),
        );
    }
}
