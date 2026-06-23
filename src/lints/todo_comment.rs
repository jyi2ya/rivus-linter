use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_TODO_COMMENT;
use super::msg::Msg;

/// Check source span for `// TODO` or `// FIXME` comments.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, span: Span) {
    let source_map = cx.tcx.sess.source_map();
    if let Ok(src) = source_map.span_to_snippet(span) {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                let lower = trimmed.to_ascii_lowercase();
                if lower.contains("todo") || lower.contains("fixme") {
                    cx.emit_span_lint(
                        RVS_TODO_COMMENT,
                        span,
                        Msg::new(span, "TODO/FIXME comment found"),
                    );
                    return;
                }
            }
        }
    }
}
