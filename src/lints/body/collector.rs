use std::collections::HashSet;

use rustc_hir::{Body, ExprKind, Mutability, def::DefKind};
use rustc_lint::LateContext;
use rustc_span::{Span, Symbol};

use super::super::utils::{
    CallObservation, CallTarget, rvs_qp, rvs_resolve_call, rvs_static_is_thread_local,
    rvs_visit_body_exprs_M,
};

#[derive(Debug, Default)]
pub(crate) struct BodyFacts {
    pub(crate) calls: Vec<CallObservation>,
    pub(crate) method_names: Vec<(String, Span)>,
    pub(crate) has_static_ref: bool,
    pub(crate) has_static_mut_ref: bool,
    pub(crate) has_thread_local_ref: bool,
    pub(crate) has_stub: bool,
}

pub(crate) fn rvs_collect_body_facts_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
) -> BodyFacts {
    let mut facts = BodyFacts::default();
    rvs_visit_body_exprs_M(cx.tcx, body.value, |expr| {
        match &expr.kind {
            ExprKind::Path(qpath) => {
                if let rustc_hir::def::Res::Def(DefKind::Static { mutability, .. }, def_id) =
                    cx.qpath_res(qpath, expr.hir_id)
                {
                    if rvs_static_is_thread_local(cx, def_id) {
                        facts.has_thread_local_ref = true;
                    }
                    match mutability {
                        Mutability::Mut => facts.has_static_mut_ref = true,
                        Mutability::Not => facts.has_static_ref = true,
                    }
                }
            }
            ExprKind::MethodCall(path, ..) => {
                facts
                    .method_names
                    .push((path.ident.name.as_str().to_string(), expr.span));
            }
            _ => {}
        }

        if matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
            && let Some(observation) = rvs_resolve_call(cx, expr)
        {
            facts.calls.push(observation);
        }
        if !facts.has_stub && rvs_expr_is_stub(expr) {
            facts.has_stub = true;
        }
    });
    facts
}

pub(crate) fn rvs_collect_test_call_names_M(facts: &BodyFacts, out: &mut HashSet<String>) {
    for observation in &facts.calls {
        let path = match &observation.target {
            CallTarget::Resolved { def_path, .. } => def_path,
            CallTarget::UnresolvedPath { path } => path,
        };
        let name = path.rsplit("::").next().unwrap_or(path);
        if name.starts_with("rvs_") {
            out.insert(name.to_string());
        }
    }
    for (name, _) in &facts.method_names {
        if name.starts_with("rvs_") {
            out.insert(name.clone());
        }
    }
}

fn rvs_expr_is_stub(expr: &rustc_hir::Expr<'_>) -> bool {
    if let ExprKind::Call(function, _) = &expr.kind
        && let ExprKind::Path(qpath) = &function.kind
    {
        let path = rvs_qp(qpath);
        let name = path.rsplit("::").next().unwrap_or(&path);
        if name == "todo" || name == "unimplemented" {
            return true;
        }
    }

    let names = [Symbol::intern("todo"), Symbol::intern("unimplemented")];
    let outer = expr.span.ctxt().outer_expn_data();
    if rvs_is_named_bang_macro(&outer.kind, &names) {
        return true;
    }
    let mut expansion = outer.parent;
    while expansion != rustc_span::ExpnId::root() {
        let data = expansion.expn_data();
        if rvs_is_named_bang_macro(&data.kind, &names) {
            return true;
        }
        expansion = data.parent;
    }
    false
}

fn rvs_is_named_bang_macro(kind: &rustc_span::ExpnKind, names: &[Symbol]) -> bool {
    matches!(kind, rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) if names.contains(name))
}
