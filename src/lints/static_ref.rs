use rustc_hir::{self, Body};
use rustc_lint::{LateContext, LintContext};

use super::msg::Msg;
use super::utils::*;
use super::{RVS_MISSING_SIDE_EFFECT, RVS_MISSING_THREAD_LOCAL, RVS_STATIC_REF};
use crate::capability::{Capability, CapabilitySet};

/// Check static/thread_local references in function body for missing capabilities.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    caps: &CapabilitySet,
) {
    let refs = rvs_scan_static_refs_M(cx, body);
    for (span, required, is_thread_local) in refs {
        if !caps.rvs_can_call(&required) {
            let missing: Vec<_> = caps
                .rvs_missing_for(&required)
                .iter()
                .map(|c| format!("{c}"))
                .collect();
            cx.emit_span_lint(
                RVS_STATIC_REF,
                span,
                Msg::new(
                    span,
                    format!(
                        "static ref requires {} but fn has {} (missing {})",
                        required,
                        caps,
                        missing.join(", ")
                    ),
                ),
            );
        }
        if required.rvs_contains(Capability::S) && !caps.rvs_contains(Capability::S) {
            cx.emit_span_lint(
                RVS_MISSING_SIDE_EFFECT,
                span,
                Msg::new(span, "reads static but suffix lacks S"),
            );
        }
        if is_thread_local && !caps.rvs_contains(Capability::T) {
            cx.emit_span_lint(
                RVS_MISSING_THREAD_LOCAL,
                span,
                Msg::new(span, "reads thread_local! but suffix lacks T"),
            );
        }
    }
}
