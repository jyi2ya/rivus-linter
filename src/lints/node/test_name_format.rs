use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::RVS_TEST_NAME_FORMAT;
use super::super::msg::rvs_emit_span_lint_S;
use super::super::utils::rvs_valid_test;

/// Check that test function names match the `test_YYYYMMDD_name` format.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, name: &str, span: Span, is_test: bool) {
    if is_test && !rvs_valid_test(name) {
        rvs_emit_span_lint_S(
            cx,
            RVS_TEST_NAME_FORMAT,
            span,
            format!("test '{name}' not test_YYYYMMDD_name"),
        );
    }
}
