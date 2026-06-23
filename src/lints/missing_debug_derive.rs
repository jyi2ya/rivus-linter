use rustc_hir::Item;
use rustc_lint::{LateContext, LintContext};

use super::RVS_MISSING_DEBUG_DERIVE;
use super::msg::Msg;
use super::utils::rvs_has_debug_derive;

/// Check pub struct/enum missing `#[derive(Debug)]`.
pub(crate) fn rvs_check_struct_or_enum_S<'tcx>(cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
    let name = cx.tcx.item_name(item.owner_id.def_id);
    if !rvs_has_debug_derive(cx, item.owner_id.def_id.into()) {
        cx.emit_span_lint(
            RVS_MISSING_DEBUG_DERIVE,
            item.span,
            Msg::new(
                item.span,
                format!("type '{}' missing #[derive(Debug)]", name),
            ),
        );
    }
}
