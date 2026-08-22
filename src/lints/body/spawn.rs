use rustc_lint::LateContext;

use super::super::RVS_SPAWN_WARNING;
use super::super::msg::rvs_emit_node_span_lint_S;
use super::super::utils::{CallSyntax, rvs_is_spawn, rvs_is_sysroot_crate_id};
use super::{BodyFacts, rvs_path_lint_callable};
use crate::lints::utils::ObservationKind;

pub(crate) fn rvs_is_sysroot_runtime_target(
    cx: &LateContext<'_>,
    target: &super::super::utils::CallTarget,
    path: &str,
) -> bool {
    let Some(crate_name) = path.split("::").next() else {
        return false;
    };
    if !matches!(crate_name, "std" | "core" | "alloc") {
        return true;
    }
    let super::super::utils::CallTarget::Resolved { crate_id, .. } = target else {
        return false;
    };
    rvs_is_sysroot_crate_id(cx, *crate_id, crate_name)
}

/// Walk function body looking for spawn calls outside of tests.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts, is_test: bool) {
    for observation in &facts.call_observations {
        if observation.kind != ObservationKind::Direct {
            continue;
        }
        let Some(path) = rvs_path_lint_callable(&observation.target) else {
            continue;
        };
        if !is_test
            && rvs_is_sysroot_runtime_target(cx, &observation.target, path.as_ref())
            && rvs_is_spawn(path.as_ref())
        {
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
