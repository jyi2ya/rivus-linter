use rustc_hir::Body;
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_EMPTY_FN;
use super::msg::Msg;
use super::utils::rvs_is_empty_body;

/// Check for empty function bodies (optionally containing only debug_assert!).
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    span: Span,
    has_body: bool,
    is_stub: bool,
) {
    if has_body && !is_stub {
        let (is_empty, only_debug_asserts) = rvs_is_empty_body(body);
        if is_empty {
            let msg = if only_debug_asserts {
                "function body contains only debug_assert!"
            } else {
                "empty function body"
            };
            cx.emit_span_lint(RVS_EMPTY_FN, span, Msg::new(span, msg));
        }
    }
}
