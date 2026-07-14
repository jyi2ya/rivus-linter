use rustc_lint::LateContext;

use super::super::RVS_ERROR_SWALLOW;
use super::super::msg::Msg;
use super::super::utils::{CallSyntax, CallTarget};
use super::BodyFacts;

const ERROR_SWALLOW_PATHS: &[&str] = &[
    "core::result::Result::ok",
    "core::result::Result::unwrap_or_default",
];

/// Walk function body looking for `.ok()` and `.unwrap_or_default()` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        let CallTarget::Resolved { def_path, .. } = &observation.target else {
            continue;
        };
        if ERROR_SWALLOW_PATHS.contains(&def_path.rvs_as_str()) {
            let name = def_path.rvs_fn_name_str();
            let call = match observation.syntax {
                CallSyntax::Method => format!(".{name}()"),
                CallSyntax::Function => format!("{name}(...)"),
            };
            cx.tcx.emit_node_span_lint(
                RVS_ERROR_SWALLOW,
                observation.hir_id,
                observation.span,
                Msg::rvs_new(observation.span, format!("{call} swallows errors")),
            );
        }
    }
}
