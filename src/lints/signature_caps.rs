use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::utils::FnInfo;
use super::{RVS_MISSING_ASYNC, RVS_MISSING_MUTABLE, RVS_MISSING_UNSAFE};
use crate::capability::{Capability, CapabilityPolicy};

/// Check that async fns have A, unsafe fns have U, &mut params have M in suffix.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, span: Span, info: &FnInfo) {
    if CapabilityPolicy::rvs_requires_signature_cap(info.facts, Capability::A)
        && !info.caps.rvs_contains(Capability::A)
    {
        cx.emit_span_lint(
            RVS_MISSING_ASYNC,
            span,
            Msg::new(span, "async but suffix lacks A"),
        );
    }
    if CapabilityPolicy::rvs_requires_signature_cap(info.facts, Capability::U)
        && !info.caps.rvs_contains(Capability::U)
    {
        cx.emit_span_lint(
            RVS_MISSING_UNSAFE,
            span,
            Msg::new(span, "unsafe code but suffix lacks U"),
        );
    }
    if CapabilityPolicy::rvs_requires_signature_cap(info.facts, Capability::M)
        && !info.caps.rvs_contains(Capability::M)
    {
        cx.emit_span_lint(
            RVS_MISSING_MUTABLE,
            span,
            Msg::new(span, "&mut param but suffix lacks M"),
        );
    }
}
