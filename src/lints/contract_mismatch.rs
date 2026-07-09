use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::RVS_CONTRACT_MISMATCH;
use super::msg::Msg;
use crate::capability::Capability;
use crate::inference::FnContractDiff;

pub(crate) fn rvs_check_contract_diff_S(
    cx: &LateContext<'_>,
    span: Span,
    diff: &FnContractDiff,
    is_test: bool,
    is_trait_impl_method: bool,
) {
    if is_test || is_trait_impl_method {
        return;
    }
    let Some(expected_name) = diff.expected_name.as_ref() else {
        return;
    };
    if !diff
        .expected_public_caps
        .as_ref()
        .is_some_and(|caps| caps.rvs_contains(Capability::P))
    {
        return;
    }
    if expected_name == &diff.actual_name {
        return;
    }
    cx.emit_span_lint(
        RVS_CONTRACT_MISMATCH,
        span,
        Msg::rvs_new(
            span,
            format!("'{}' should be named '{expected_name}'", diff.actual_name),
        ),
    );
}
