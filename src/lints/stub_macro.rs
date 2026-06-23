use rustc_hir::Body;
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_STUB_MACRO;
use super::msg::Msg;
use super::utils::rvs_scan_stub;

/// Check for `todo!()`/`unimplemented!()` stub macros in function body.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, span: Span) -> bool {
    let is_stub = rvs_scan_stub(cx.tcx, body);
    if is_stub {
        cx.emit_span_lint(
            RVS_STUB_MACRO,
            span,
            Msg::new(span, "stub: todo!()/unimplemented!()"),
        );
    }
    is_stub
}
