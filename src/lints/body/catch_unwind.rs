use rustc_lint::{LateContext, LintContext};

use super::super::RVS_CATCH_UNWIND;
use super::super::msg::Msg;
use super::super::utils::{CallSyntax, CallTarget};
use super::BodyFacts;

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for observation in &facts.calls {
        let is_catch_unwind = match (observation.syntax, &observation.target) {
            (CallSyntax::Method, CallTarget::Resolved { def_path, .. }) => {
                def_path.rsplit("::").next() == Some("catch_unwind")
            }
            (CallSyntax::Method, CallTarget::UnresolvedMethod { name }) => name == "catch_unwind",
            (
                CallSyntax::Function,
                CallTarget::Resolved {
                    def_path,
                    def_kind:
                        rustc_hir::def::DefKind::Fn
                        | rustc_hir::def::DefKind::AssocFn
                        | rustc_hir::def::DefKind::Variant,
                },
            ) => def_path.rsplit("::").next() == Some("catch_unwind"),
            _ => false,
        };
        if is_catch_unwind {
            cx.emit_span_lint(
                RVS_CATCH_UNWIND,
                observation.span,
                Msg::rvs_new(observation.span, "catch_unwind — fix panic source instead"),
            );
        }
    }
}
