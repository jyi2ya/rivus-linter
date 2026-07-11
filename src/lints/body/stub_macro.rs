use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::super::RVS_STUB_MACRO;
use super::super::msg::Msg;
use super::BodyFacts;

/// Check for `todo!()`/`unimplemented!()` stub macros in function body.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts, span: Span) -> bool {
    if facts.has_stub {
        cx.emit_span_lint(
            RVS_STUB_MACRO,
            span,
            Msg::rvs_new(span, "stub: todo!()/unimplemented!()"),
        );
    }
    facts.has_stub
}
