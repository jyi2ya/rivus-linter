use rustc_hir::{Impl, Item};
use rustc_lint::LateContext;
use rustc_span::sym;

use super::super::msg::rvs_emit_span_lint_S;
use super::super::{RVS_DEREF_POLYMORPHISM, RVS_INTO_IMPL};

/// Check `impl` items for Into and Deref trait implementations.
pub(crate) fn rvs_check_impl_S<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    imp: &Impl<'tcx>,
) {
    if let Some(trait_ref) = &imp.of_trait {
        if let Some(did) = trait_ref.trait_ref.trait_def_id() {
            if cx.tcx.is_diagnostic_item(sym::Into, did) {
                rvs_emit_span_lint_S(
                    cx,
                    RVS_INTO_IMPL,
                    item.span,
                    "impl Into — implement From instead (Into is auto-provided)",
                );
            }
            if cx.tcx.lang_items().deref_trait() == Some(did) {
                rvs_emit_span_lint_S(
                    cx,
                    RVS_DEREF_POLYMORPHISM,
                    item.span,
                    "impl Deref — use composition instead of Deref polymorphism",
                );
            }
        }
    }
}
