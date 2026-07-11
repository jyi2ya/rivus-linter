use rustc_lint::{LateContext, LintContext};

use super::super::RVS_SPAWN_WARNING;
use super::super::msg::Msg;
use super::super::utils::{CallSyntax, CallTarget, rvs_is_spawn_S};
use super::BodyFacts;

/// Walk function body looking for spawn calls outside of tests.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts, is_test: bool) {
    for observation in &facts.calls {
        let path = match &observation.target {
            CallTarget::Resolved {
                def_path,
                def_kind:
                    rustc_hir::def::DefKind::Fn
                    | rustc_hir::def::DefKind::AssocFn
                    | rustc_hir::def::DefKind::Variant,
            } => def_path,
            CallTarget::UnresolvedPath { path } => path,
            CallTarget::Resolved { .. } => continue,
        };
        if !is_test && rvs_is_spawn_S(path) {
            let message = match observation.syntax {
                CallSyntax::Function => {
                    format!("spawn: {path} — use structured concurrency")
                }
                CallSyntax::Method => format!("spawn: {path}"),
            };
            cx.emit_span_lint(
                RVS_SPAWN_WARNING,
                observation.span,
                Msg::rvs_new(observation.span, message),
            );
        }
    }
}
