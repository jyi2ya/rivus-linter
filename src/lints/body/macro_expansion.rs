use rustc_span::{Span, Symbol};

pub(crate) fn rvs_span_has_bang_macro(span: Span, names: &[Symbol]) -> bool {
    let outer = span.ctxt().outer_expn_data();
    if rvs_is_named_bang_macro(&outer.kind, names) {
        return true;
    }
    let mut expansion = outer.parent;
    while expansion != rustc_span::ExpnId::root() {
        let data = expansion.expn_data();
        if rvs_is_named_bang_macro(&data.kind, names) {
            return true;
        }
        expansion = data.parent;
    }
    false
}

fn rvs_is_named_bang_macro(kind: &rustc_span::ExpnKind, names: &[Symbol]) -> bool {
    matches!(kind, rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) if names.contains(name))
}
