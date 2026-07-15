use rustc_hir::{self};
use rustc_span::Symbol;

use super::super::RVS_DENY_WARNINGS;
use super::super::msg::rvs_emit_span_lint_S;

/// `check_crate` — detect `#![deny(warnings)]` at crate level.
pub(crate) fn rvs_check_crate_S(cx: &rustc_lint::LateContext<'_>) {
    let attrs = cx.tcx.hir_attrs(rustc_hir::CRATE_HIR_ID);
    let deny_sym = Symbol::intern("deny");
    let warnings_sym = Symbol::intern("warnings");
    for a in attrs {
        if a.name() == Some(deny_sym) {
            if let Some(items) = a.meta_item_list() {
                for m in &items {
                    if let Some(p) = m.ident() {
                        if p.name == warnings_sym {
                            rvs_emit_span_lint_S(
                                cx,
                                RVS_DENY_WARNINGS,
                                a.span(),
                                "#![deny(warnings)] — use named lints instead",
                            );
                        }
                    }
                }
            }
        }
    }
}
