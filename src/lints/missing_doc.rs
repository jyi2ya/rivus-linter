use rustc_hir;
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_MISSING_DOC;
use super::msg::Msg;
use super::utils::{rvs_has_any_doc, rvs_has_attr};

/// Check that pub rvs_ functions have `///` doc comments.
pub(crate) fn rvs_check_fn_S(
    cx: &LateContext<'_>,
    name: &str,
    span: Span,
    attrs: &[rustc_hir::Attribute],
    is_pub: bool,
) {
    if !is_pub {
        return;
    }
    if !name.starts_with("rvs_") {
        return;
    }
    if rvs_has_attr(attrs, "test") {
        return;
    }
    if !rvs_has_any_doc(attrs) {
        cx.emit_span_lint(
            RVS_MISSING_DOC,
            span,
            Msg::new(span, format!("pub fn '{name}' missing /// doc comment")),
        );
    }
}
