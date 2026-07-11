use rustc_lint::{LateContext, LintContext};

use super::super::RVS_ERROR_SWALLOW;
use super::super::msg::Msg;
use super::super::utils::ERROR_SWALLOW_METHODS;
use super::BodyFacts;

/// Walk function body looking for `.ok()` and `.unwrap_or_default()` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for (name, span) in &facts.method_names {
        if ERROR_SWALLOW_METHODS.contains(&name.as_str()) {
            cx.emit_span_lint(
                RVS_ERROR_SWALLOW,
                *span,
                Msg::rvs_new(*span, format!(".{name}() swallows errors")),
            );
        }
    }
}
