use rustc_lint::{LateContext, LintContext};

use super::super::RVS_ERROR_SWALLOW;
use super::super::msg::Msg;
use super::super::utils::{CallSyntax, CallTarget, ERROR_SWALLOW_METHODS};
use super::BodyFacts;

/// Walk function body looking for `.ok()` and `.unwrap_or_default()` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        if observation.syntax != CallSyntax::Method {
            continue;
        }
        let name = match &observation.target {
            CallTarget::Resolved { def_path, .. } => def_path.rvs_fn_name_str(),
            CallTarget::UnresolvedMethod { name } => name,
            CallTarget::UnresolvedPath { .. } => continue,
        };
        if ERROR_SWALLOW_METHODS.contains(&name) {
            cx.emit_span_lint(
                RVS_ERROR_SWALLOW,
                observation.span,
                Msg::rvs_new(observation.span, format!(".{name}() swallows errors")),
            );
        }
    }
}
