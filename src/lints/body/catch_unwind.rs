use rustc_lint::LateContext;

use super::super::RVS_CATCH_UNWIND;
use super::super::msg::rvs_emit_node_span_lint_S;
use super::super::utils::CallTarget;
use super::BodyFacts;
use super::spawn::rvs_is_sysroot_runtime_target;
use crate::lints::utils::ObservationKind;

const CATCH_UNWIND_PATHS: &[&str] = &[
    "core::intrinsics::catch_unwind",
    "std::panic::catch_unwind",
    "std::panicking::catch_unwind",
];

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.call_observations {
        if observation.kind != ObservationKind::Direct {
            continue;
        }
        let is_catch_unwind = match &observation.target {
            CallTarget::Resolved { def_path, .. } => {
                let path = def_path.rvs_user_path();
                CATCH_UNWIND_PATHS.contains(&path.as_ref())
                    && rvs_is_sysroot_runtime_target(cx, &observation.target, path.as_ref())
            }
            CallTarget::UnresolvedMethod { .. } | CallTarget::UnresolvedPath { .. } => false,
        };
        if is_catch_unwind {
            rvs_emit_node_span_lint_S(
                cx,
                RVS_CATCH_UNWIND,
                observation.hir_id,
                observation.span,
                "catch_unwind — fix panic source instead",
            );
        }
    }
}
