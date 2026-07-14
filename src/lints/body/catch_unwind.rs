use rustc_lint::LateContext;

use super::super::RVS_CATCH_UNWIND;
use super::super::msg::Msg;
use super::super::utils::CallTarget;
use super::BodyFacts;

const CATCH_UNWIND_PATHS: &[&str] = &[
    "core::intrinsics::catch_unwind",
    "std::panic::catch_unwind",
    "std::panicking::catch_unwind",
];

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        let is_catch_unwind = match &observation.target {
            CallTarget::Resolved { def_path, .. } => {
                CATCH_UNWIND_PATHS.contains(&def_path.rvs_as_str())
            }
            CallTarget::UnresolvedMethod { .. } | CallTarget::UnresolvedPath { .. } => false,
        };
        if is_catch_unwind {
            cx.tcx.emit_node_span_lint(
                RVS_CATCH_UNWIND,
                observation.hir_id,
                observation.span,
                Msg::rvs_new(observation.span, "catch_unwind — fix panic source instead"),
            );
        }
    }
}
