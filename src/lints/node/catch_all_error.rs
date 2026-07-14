use rustc_hir::{EnumDef, Item};
use rustc_lint::LateContext;

use super::super::RVS_CATCH_ALL_ERROR_VARIANT;
use super::super::msg::Msg;
use super::super::utils::{CATCH_ALL_VARIANT_NAMES, rvs_has_attr};

/// Check error enums for catch-all variants (Unknown/Other/etc.).
pub(crate) fn rvs_check_enum_S<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    enum_def: &EnumDef<'tcx>,
) {
    let attrs = cx.tcx.hir_attrs(item.hir_id());
    let name = cx.tcx.item_name(item.owner_id.def_id);
    let name_s = name.as_str();
    if name_s.contains("Error") || rvs_has_attr(attrs, "error") {
        for v in enum_def.variants {
            let vn = v.ident.name.as_str();
            if CATCH_ALL_VARIANT_NAMES.contains(&vn) {
                cx.tcx.emit_node_span_lint(
                    RVS_CATCH_ALL_ERROR_VARIANT,
                    v.hir_id,
                    v.span,
                    Msg::rvs_new(v.span, format!("catch-all variant '{vn}' in {name_s}")),
                );
            }
        }
    }
}
