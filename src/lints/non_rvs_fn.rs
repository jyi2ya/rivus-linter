use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_NON_RVS_FN;
use super::msg::Msg;

/// Check for functions missing the `rvs_` prefix.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, name: &str, span: Span) {
    if name.starts_with(|c: char| c.is_ascii_lowercase()) {
        cx.emit_span_lint(
            RVS_NON_RVS_FN,
            span,
            Msg::new(span, format!("'{name}' missing rvs_ prefix")),
        );
    }
}
