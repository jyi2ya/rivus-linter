use rustc_middle::ty::TyCtxt;
use rustc_span::{ExpnData, Span, Symbol};

pub(crate) fn rvs_span_has_bang_macro(tcx: TyCtxt<'_>, span: Span, names: &[Symbol]) -> bool {
    let outer = span.ctxt().outer_expn_data();
    if rvs_is_named_bang_macro(tcx, &outer, names) {
        return true;
    }
    let mut expansion = outer.parent;
    while expansion != rustc_span::ExpnId::root() {
        let data = expansion.expn_data();
        if rvs_is_named_bang_macro(tcx, &data, names) {
            return true;
        }
        expansion = data.parent;
    }
    false
}

fn rvs_is_named_bang_macro(tcx: TyCtxt<'_>, data: &ExpnData, names: &[Symbol]) -> bool {
    let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = data.kind else {
        return false;
    };
    if !names.contains(&name) {
        return false;
    }
    let Some(def_id) = data.macro_def_id else {
        return false;
    };
    tcx.lang_items()
        .sized_trait()
        .is_some_and(|sized_trait| def_id.krate == sized_trait.krate)
}
