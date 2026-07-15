use rustc_lint::LateContext;

use super::super::RVS_SPAWN_WARNING;
use super::super::msg::rvs_emit_node_span_lint_S;
use super::super::utils::{CallSyntax, rvs_is_spawn_S};
use super::{BodyFacts, rvs_path_lint_callable};

/// Walk function body looking for spawn calls outside of tests.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts, is_test: bool) {
    for observation in &facts.calls {
        let Some(path) = rvs_path_lint_callable(&observation.target) else {
            continue;
        };
        if !is_test && rvs_is_spawn_S(path.as_ref()) {
            let message = match observation.syntax {
                CallSyntax::Function => {
                    format!("spawn: {path} — use structured concurrency")
                }
                CallSyntax::Method => format!("spawn: {path}"),
            };
            rvs_emit_node_span_lint_S(
                cx,
                RVS_SPAWN_WARNING,
                observation.hir_id,
                observation.span,
                message,
            );
        }
    }
}
