use rustc_lint::LateContext;

use super::super::RVS_REFLECTION_USAGE;
use super::super::msg::Msg;
use super::super::utils::rvs_is_reflection_S;
use super::{BodyFacts, rvs_path_lint_callable};

/// Walk function body looking for reflection usage (type_name, type_id, Any).
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        let Some(path) = rvs_path_lint_callable(&observation.target) else {
            continue;
        };
        if rvs_is_reflection_S(path) {
            cx.tcx.emit_node_span_lint(
                RVS_REFLECTION_USAGE,
                observation.hir_id,
                observation.span,
                Msg::rvs_new(observation.span, "reflection — use trait dispatch instead"),
            );
        }
    }
}
