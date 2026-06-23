use rustc_hir::{self, HeaderSafety, HirId, Safety};
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_MISSING_SAFETY_DOC;
use super::msg::Msg;
use super::utils::rvs_has_doc_section;

/// Check that unsafe functions have `/// # Safety` doc section.
pub(crate) fn rvs_check_fn_S(
    cx: &LateContext<'_>,
    hir_id: HirId,
    span: Span,
    safety: &HeaderSafety,
) {
    if !matches!(safety, HeaderSafety::Normal(Safety::Unsafe)) {
        return;
    }
    if !rvs_has_doc_section(cx, hir_id, "Safety") {
        cx.emit_span_lint(
            RVS_MISSING_SAFETY_DOC,
            span,
            Msg::new(span, "unsafe fn missing /// # Safety"),
        );
    }
}
