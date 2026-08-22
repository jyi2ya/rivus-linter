use rustc_lint::LateContext;

use super::super::RVS_REFLECTION_USAGE;
use super::super::msg::rvs_emit_node_span_lint_S;
use super::super::utils::rvs_is_reflection;
use super::spawn::rvs_is_sysroot_runtime_target;
use super::{BodyFacts, rvs_path_lint_callable};
use crate::lints::utils::ObservationKind;

/// Walk function body looking for reflection usage (type_name, type_id, Any).
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.call_observations {
        if observation.kind != ObservationKind::Direct {
            continue;
        }
        let Some(path) = rvs_path_lint_callable(&observation.target) else {
            continue;
        };
        if rvs_is_sysroot_runtime_target(cx, &observation.target, path.as_ref())
            && rvs_is_reflection(path.as_ref())
        {
            rvs_emit_node_span_lint_S(
                cx,
                RVS_REFLECTION_USAGE,
                observation.hir_id,
                observation.span,
                "reflection — use trait dispatch instead",
            );
        }
    }
}
