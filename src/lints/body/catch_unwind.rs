use rustc_lint::{LateContext, LintContext};

use super::super::RVS_CATCH_UNWIND;
use super::super::msg::Msg;
use super::super::utils::{CallSyntax, CallTarget};
use super::BodyFacts;

/// Walk function body looking for `catch_unwind` calls.
pub(crate) fn rvs_check_fn_S<'tcx>(cx: &LateContext<'tcx>, facts: &BodyFacts) {
    for (name, span) in &facts.method_names {
        if name == "catch_unwind" {
            cx.emit_span_lint(
                RVS_CATCH_UNWIND,
                *span,
                Msg::rvs_new(*span, "catch_unwind — fix panic source instead"),
            );
        }
    }
    for observation in &facts.calls {
        if observation.syntax != CallSyntax::Function {
            continue;
        }
        let CallTarget::Resolved {
            def_path,
            def_kind:
                rustc_hir::def::DefKind::Fn
                | rustc_hir::def::DefKind::AssocFn
                | rustc_hir::def::DefKind::Variant,
        } = &observation.target
        else {
            continue;
        };
        if def_path.rsplit("::").next() == Some("catch_unwind") {
            cx.emit_span_lint(
                RVS_CATCH_UNWIND,
                observation.span,
                Msg::rvs_new(observation.span, "catch_unwind — fix panic source instead"),
            );
        }
    }
}
