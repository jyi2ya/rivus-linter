use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_TEST_NAME_FORMAT;
use super::msg::Msg;
use super::utils::rvs_valid_test;

/// Check that test function names match the `test_YYYYMMDD_name` format.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, name: &str, span: Span, is_test: bool) {
    if is_test && !rvs_valid_test(name) {
        cx.emit_span_lint(
            RVS_TEST_NAME_FORMAT,
            span,
            Msg::new(span, format!("test '{name}' not test_YYYYMMDD_name")),
        );
    }
}
