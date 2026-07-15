use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::RVS_STUB_MACRO;
use super::super::msg::rvs_emit_span_lint_S;
use super::BodyFacts;

/// Check for `todo!()`/`unimplemented!()` stub macros in function body.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts, span: Span) -> bool {
    if facts.has_stub {
        rvs_emit_span_lint_S(cx, RVS_STUB_MACRO, span, "stub: todo!()/unimplemented!()");
    }
    facts.has_stub
}
