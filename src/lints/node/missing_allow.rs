use rustc_hir;
use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::RVS_MISSING_ALLOW;
use super::super::msg::rvs_emit_span_lint_S;
use super::super::utils::rvs_allows_non_snake_case;

/// Check for uppercase suffix without `#[allow(non_snake_case)]`.
pub(crate) fn rvs_check_fn_S(
    cx: &LateContext<'_>,
    hir_id: rustc_hir::HirId,
    span: Span,
    raw_suffix: &str,
) {
    if !raw_suffix.is_empty() && !rvs_allows_non_snake_case(cx, hir_id) {
        rvs_emit_span_lint_S(
            cx,
            RVS_MISSING_ALLOW,
            span,
            "uppercase suffix without #[allow(non_snake_case)]",
        );
    }
}
