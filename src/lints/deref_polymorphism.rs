use rustc_hir::{Impl, Item};
use rustc_lint::{LateContext, LintContext};

use super::msg::Msg;
use super::{RVS_DEREF_POLYMORPHISM, RVS_INTO_IMPL};

/// Check `impl` items for Into and Deref trait implementations.
pub(crate) fn rvs_check_impl_S<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    imp: &Impl<'tcx>,
) {
    if let Some(trait_ref) = &imp.of_trait {
        if let Some(did) = trait_ref.trait_ref.trait_def_id() {
            let trait_name = cx.tcx.item_name(did);
            if trait_name.as_str() == "Into" {
                cx.emit_span_lint(
                    RVS_INTO_IMPL,
                    item.span,
                    Msg::new(
                        item.span,
                        "impl Into — implement From instead (Into is auto-provided)",
                    ),
                );
            }
            if trait_name.as_str() == "Deref" {
                cx.emit_span_lint(
                    RVS_DEREF_POLYMORPHISM,
                    item.span,
                    Msg::new(
                        item.span,
                        "impl Deref — use composition instead of Deref polymorphism",
                    ),
                );
            }
        }
    }
}
