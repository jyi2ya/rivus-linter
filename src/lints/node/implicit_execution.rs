use rustc_hir::{Impl, Item, LangItem};
use rustc_lint::LateContext;

use super::super::RVS_UNSUPPORTED_IMPLICIT_EXECUTION;
use super::super::msg::rvs_emit_span_lint_S;

pub(crate) fn rvs_check_impl_S<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    imp: &Impl<'tcx>,
) {
    let Some(trait_def_id) = imp
        .of_trait
        .as_ref()
        .and_then(|trait_ref| trait_ref.trait_ref.trait_def_id())
    else {
        return;
    };
    let is_operator = rustc_hir::lang_items::OPERATORS
        .iter()
        .copied()
        .any(|lang_item| cx.tcx.is_lang_item(trait_def_id, lang_item))
        || cx.tcx.is_lang_item(trait_def_id, LangItem::Deref)
        || cx.tcx.is_lang_item(trait_def_id, LangItem::DerefMut);
    if is_operator {
        rvs_emit_span_lint_S(
            cx,
            RVS_UNSUPPORTED_IMPLICIT_EXECUTION,
            item.span,
            "custom operator or indexing trait implementation cannot be represented in the Rivus callgraph",
        );
    }
    if cx.tcx.is_lang_item(trait_def_id, LangItem::Drop) {
        rvs_emit_span_lint_S(
            cx,
            RVS_UNSUPPORTED_IMPLICIT_EXECUTION,
            item.span,
            "custom Drop executes implicitly and cannot be represented in the Rivus callgraph",
        );
    }
}
