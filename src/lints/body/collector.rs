use std::collections::{BTreeSet, HashSet};

use rustc_hir::{Body, ExprKind, Mutability, def::DefKind};
use rustc_lint::LateContext;
use rustc_span::Symbol;

use super::super::utils::{
    CallObservation, CallTarget, rvs_collect_all_idents_M, rvs_qp, rvs_resolve_call,
    rvs_root_body_expr, rvs_static_is_thread_local, rvs_visit_body_exprs_M,
};
use super::macro_expansion::rvs_span_has_bang_macro;
use crate::lints::ctx::TestCallTarget;
use crate::symbols::DefPath;

#[derive(Debug, Default)]
pub(crate) struct BodyFacts {
    pub(crate) calls: Vec<CallObservation>,
    pub(crate) has_static_ref: bool,
    pub(crate) has_static_mut_ref: bool,
    pub(crate) has_thread_local_ref: bool,
    pub(crate) has_stub: bool,
    pub(crate) debug_assert_identifiers: BTreeSet<String>,
}

pub(crate) fn rvs_collect_body_facts_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
) -> BodyFacts {
    let mut facts = BodyFacts::default();
    let debug_assert_macros = [
        Symbol::intern("debug_assert"),
        Symbol::intern("debug_assert_eq"),
        Symbol::intern("debug_assert_ne"),
    ];
    let root_expr = rvs_root_body_expr(cx.tcx, body);
    rvs_visit_body_exprs_M(cx.tcx, root_expr, |expr, nested_body_depth| {
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
        if nested_body_depth == 0
            && expr.span.from_expansion()
            && rvs_span_has_bang_macro(expr.span, &debug_assert_macros)
        {
            rvs_collect_all_idents_M(expr, &mut facts.debug_assert_identifiers);
        }
    });
    facts
}

pub(crate) fn rvs_collect_test_calls_M(facts: &BodyFacts, out: &mut HashSet<TestCallTarget>) {
    for observation in &facts.calls {
        let (name, target) = match &observation.target {
            CallTarget::Resolved { def_path, .. } => (
                def_path.rsplit("::").next().unwrap_or(def_path),
                TestCallTarget::Resolved(DefPath::rvs_new(def_path.clone())),
            ),
            CallTarget::UnresolvedPath { path } => {
                let name = path.rsplit("::").next().unwrap_or(path);
                (name, TestCallTarget::UnresolvedName(name.to_string()))
            }
            CallTarget::UnresolvedMethod { name } => {
                (name.as_str(), TestCallTarget::UnresolvedName(name.clone()))
            }
        };
        if name.starts_with("rvs_") {
            out.insert(target);
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
    rvs_span_has_bang_macro(expr.span, &names)
}
