use rustc_lint::LateContext;

use super::super::RVS_ERROR_SWALLOW;
use super::super::msg::rvs_emit_node_span_lint_S;
use super::super::utils::CallSyntax;
use super::BodyFacts;

/// Check calls that discard a `Result` without handling its error.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for (hir_id, span, syntax, name) in &facts.result_swallow_calls {
        let call = match syntax {
            CallSyntax::Method => format!(".{name}()"),
            CallSyntax::Function => format!("{name}(...)"),
        };
        rvs_emit_node_span_lint_S(
            cx,
            RVS_ERROR_SWALLOW,
            *hir_id,
            *span,
            format!("{call} swallows errors"),
        );
    }
    for (hir_id, span) in &facts.result_drop_calls {
        rvs_emit_node_span_lint_S(
            cx,
            RVS_ERROR_SWALLOW,
            *hir_id,
            *span,
            "drop(Result) discards a Result without handling it".to_string(),
        );
    }
}
