use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::{RVS_MISSING_ASYNC, RVS_MISSING_MUTABLE, RVS_MISSING_UNSAFE};
use crate::inference::{FnContractMismatch, FnContractMismatchKind};

pub(crate) fn rvs_check_contract_mismatches_S(
    cx: &LateContext<'_>,
    span: Span,
    mismatches: &[FnContractMismatch],
) {
    let has_kind = |kind| mismatches.iter().any(|mismatch| mismatch.kind == kind);
    if has_kind(FnContractMismatchKind::MissingAsync) {
        cx.emit_span_lint(
            RVS_MISSING_ASYNC,
            span,
            Msg::rvs_new(span, "async but suffix lacks A"),
        );
    }
    if has_kind(FnContractMismatchKind::MissingUnsafe) {
        cx.emit_span_lint(
            RVS_MISSING_UNSAFE,
            span,
            Msg::rvs_new(span, "unsafe code but suffix lacks U"),
        );
    }
    if has_kind(FnContractMismatchKind::MissingMutable) {
        cx.emit_span_lint(
            RVS_MISSING_MUTABLE,
            span,
            Msg::rvs_new(span, "&mut param but suffix lacks M"),
        );
    }
}
