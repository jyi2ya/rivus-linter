use rustc_hir::Item;
use rustc_lint::LateContext;

use super::super::RVS_MISSING_DEBUG_DERIVE;
use super::super::msg::rvs_emit_span_lint_S;
use super::super::utils::rvs_has_debug_derive;

/// Check pub struct/enum missing `#[derive(Debug)]`.
pub(crate) fn rvs_check_struct_or_enum_S<'tcx>(cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
    if !cx.tcx.visibility(item.owner_id.def_id).is_public() {
        return;
    }
    let name = cx.tcx.item_name(item.owner_id.def_id);
    if !rvs_has_debug_derive(cx, item.owner_id.def_id.into()) {
        rvs_emit_span_lint_S(
            cx,
            RVS_MISSING_DEBUG_DERIVE,
            item.span,
            format!("type '{}' missing #[derive(Debug)]", name),
        );
    }
}
