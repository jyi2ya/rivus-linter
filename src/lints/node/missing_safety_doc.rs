use rustc_hir::{self, HeaderSafety, HirId, Safety};
use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::RVS_MISSING_SAFETY_DOC;
use super::super::msg::rvs_emit_span_lint_S;
use super::super::utils::rvs_has_doc_section;

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
        rvs_emit_span_lint_S(
            cx,
            RVS_MISSING_SAFETY_DOC,
            span,
            "unsafe fn missing /// # Safety",
        );
    }
}
