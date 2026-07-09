use rustc_hir::{self, Body};
use rustc_lint::{LateContext, LintContext};

use super::msg::Msg;
use super::utils::rvs_scan_static_refs_M;
use super::{
    RVS_MISSING_SIDE_EFFECT, RVS_MISSING_THREAD_LOCAL, RVS_MISSING_UNSAFE, RVS_STATIC_REF,
};
use crate::capability::{Capability, CapabilitySet};

/// Check static/thread_local references in function body for missing capabilities.
pub(crate) fn rvs_check_fn_MS<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    caps: &CapabilitySet,
) {
    let refs = rvs_scan_static_refs_M(cx, body);
    for (span, required, is_thread_local) in refs {
        let missing_caps: Vec<_> = required
            .rvs_iter()
            .filter(|cap| !caps.rvs_contains(*cap))
            .collect();
        if !missing_caps.is_empty() {
            let missing: Vec<_> = missing_caps.iter().map(|c| format!("{c}")).collect();
            cx.emit_span_lint(
                RVS_STATIC_REF,
                span,
                Msg::rvs_new(
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
                Msg::rvs_new(span, "reads static but suffix lacks S"),
            );
        }
        if is_thread_local && !caps.rvs_contains(Capability::T) {
            cx.emit_span_lint(
                RVS_MISSING_THREAD_LOCAL,
                span,
                Msg::rvs_new(span, "reads thread_local! but suffix lacks T"),
            );
        }
        if required.rvs_contains(Capability::U) && !caps.rvs_contains(Capability::U) {
            cx.emit_span_lint(
                RVS_MISSING_UNSAFE,
                span,
                Msg::rvs_new(span, "static mut access but suffix lacks U"),
            );
        }
    }
}
