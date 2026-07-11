use rustc_hir::{self, Body, PatKind};
use rustc_lint::{LateContext, LintContext};

use super::super::RVS_MISSING_DEBUG_ASSERT;
use super::super::msg::Msg;
use super::BodyFacts;

/// Check that primitive numeric parameters have corresponding `debug_assert!`
/// calls referencing them.
pub(crate) fn rvs_check_fn_MS<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, facts: &BodyFacts) {
    let owner = body.value.hir_id.owner;
    let tck = cx.tcx.typeck(owner.def_id);
    let mut prims = Vec::new();
    for p in body.params {
        let ty = tck.pat_ty(p.pat);
        let ts = ty.to_string();
        if matches!(
            ts.as_str(),
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
        ) {
            if let PatKind::Binding(_, _, id, _) = p.pat.kind {
                prims.push(id.name.to_string());
            }
        }
    }
    if prims.is_empty() {
        return;
    }
    for p in &prims {
        if !facts.debug_assert_identifiers.contains(p) {
            cx.emit_span_lint(
                RVS_MISSING_DEBUG_ASSERT,
                body.value.span,
                Msg::rvs_new(
                    body.value.span,
                    format!("param '{p}' missing debug_assert!"),
                ),
            );
        }
    }
}
