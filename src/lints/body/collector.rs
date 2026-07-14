use std::collections::{HashMap, HashSet};

use rustc_hir::{Body, ExprKind, HirId, LocalSource, Mutability, PatKind, def::DefKind};
use rustc_lint::LateContext;
use rustc_middle::ty::TyKind;
use rustc_span::{Span, Symbol, sym};

use super::super::utils::{
    CallObservation, CallTarget, rvs_collect_local_bindings_M, rvs_resolve_call,
    rvs_root_body_expr, rvs_static_is_thread_local, rvs_visit_body_exprs_M,
};
use super::macro_expansion::rvs_span_has_bang_macro;
use crate::lints::ctx::TestCallTarget;

#[derive(Debug, Default)]
pub(crate) struct BodyFacts {
    pub(crate) calls: Vec<CallObservation>,
    pub(crate) has_static_ref: bool,
    pub(crate) has_static_mut_ref: bool,
    pub(crate) has_thread_local_ref: bool,
    pub(crate) has_stub: bool,
    pub(crate) debug_assert_bindings: HashSet<HirId>,
    pub(crate) result_swallow_calls: Vec<(HirId, Span, super::super::utils::CallSyntax, String)>,
    pub(crate) result_drop_calls: Vec<(HirId, Span)>,
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
    let async_param_aliases = rvs_async_param_aliases(cx, root_expr);
    rvs_visit_body_exprs_M(cx.tcx, root_expr, |expr, nested_body| {
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
            ExprKind::InlineAsm(asm) => {
                for (operand, _) in asm.operands {
                    let rustc_hir::InlineAsmOperand::SymStatic { def_id, .. } = operand else {
                        continue;
                    };
                    if rvs_static_is_thread_local(cx, *def_id) {
                        facts.has_thread_local_ref = true;
                    }
                    if let DefKind::Static { mutability, .. } = cx.tcx.def_kind(*def_id) {
                        match mutability {
                            Mutability::Mut => facts.has_static_mut_ref = true,
                            Mutability::Not => facts.has_static_ref = true,
                        }
                    }
                }
            }
            _ => {}
        }

        if matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
            && let Some(observation) = rvs_resolve_call(cx, expr)
        {
            if let Some(name) = rvs_result_swallow_name(cx, &observation.target) {
                facts.result_swallow_calls.push((
                    expr.hir_id,
                    expr.span,
                    observation.syntax,
                    name.to_string(),
                ));
            }
            if rvs_is_result_drop(cx, expr, &observation.target) {
                facts.result_drop_calls.push((expr.hir_id, expr.span));
            }
            facts.calls.push(observation);
        }
        if !facts.has_stub && rvs_expr_is_stub(cx, expr) {
            facts.has_stub = true;
        }
        if !nested_body
            && expr.span.from_expansion()
            && rvs_span_has_bang_macro(cx.tcx, expr.span, &debug_assert_macros)
        {
            let mut bindings = HashSet::new();
            rvs_collect_local_bindings_M(cx, expr, &mut bindings);
            facts
                .debug_assert_bindings
                .extend(bindings.into_iter().map(|binding| {
                    async_param_aliases
                        .get(&binding)
                        .copied()
                        .unwrap_or(binding)
                }));
        }
    });
    facts
}

fn rvs_async_param_aliases(
    cx: &LateContext<'_>,
    root_expr: &rustc_hir::Expr<'_>,
) -> HashMap<HirId, HirId> {
    let ExprKind::Block(block, _) = root_expr.kind else {
        return HashMap::new();
    };
    block
        .stmts
        .iter()
        .filter_map(|statement| {
            let rustc_hir::StmtKind::Let(local) = statement.kind else {
                return None;
            };
            if !matches!(local.source, LocalSource::AsyncFn) {
                return None;
            }
            let PatKind::Binding(_, alias_hir_id, _, _) = local.pat.kind else {
                return None;
            };
            let ExprKind::Path(qpath) = local.init?.kind else {
                return None;
            };
            let rustc_hir::def::Res::Local(parameter_hir_id) =
                cx.qpath_res(&qpath, local.init?.hir_id)
            else {
                return None;
            };
            Some((alias_hir_id, parameter_hir_id))
        })
        .collect()
}

fn rvs_result_swallow_name(cx: &LateContext<'_>, target: &CallTarget) -> Option<&'static str> {
    let CallTarget::Resolved {
        def_path, crate_id, ..
    } = target
    else {
        return None;
    };
    let path = def_path.rvs_user_path();
    let name = match path.as_ref() {
        "core::result::Result::ok" => "ok",
        "core::result::Result::unwrap_or_default" => "unwrap_or_default",
        _ => return None,
    };
    let result_def_id = cx.tcx.get_diagnostic_item(sym::Result)?;
    (*crate_id == cx.tcx.stable_crate_id(result_def_id.krate).as_u64()).then_some(name)
}

fn rvs_is_result_drop<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx rustc_hir::Expr<'tcx>,
    target: &CallTarget,
) -> bool {
    let ExprKind::Call(_, arguments) = &expr.kind else {
        return false;
    };
    let CallTarget::Resolved {
        def_path, crate_id, ..
    } = target
    else {
        return false;
    };
    let Some(drop_def_id) = cx.tcx.get_diagnostic_item(sym::mem_drop) else {
        return false;
    };
    if def_path.rvs_user_path() != "core::mem::drop"
        || *crate_id != cx.tcx.stable_crate_id(drop_def_id.krate).as_u64()
    {
        return false;
    }
    let [argument] = *arguments else {
        return false;
    };
    rvs_is_std_result_expr(cx, expr.hir_id.owner.def_id, argument)
}

fn rvs_is_std_result_expr<'tcx>(
    cx: &LateContext<'tcx>,
    owner: rustc_hir::def_id::LocalDefId,
    expr: &'tcx rustc_hir::Expr<'tcx>,
) -> bool {
    let expr_type = cx.tcx.typeck(owner).expr_ty(expr);
    let expr_type = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), expr_type)
        .unwrap_or(expr_type);
    matches!(
        expr_type.kind(),
        TyKind::Adt(adt, _) if cx.tcx.is_diagnostic_item(sym::Result, adt.did())
    )
}

pub(crate) fn rvs_collect_test_calls_M(facts: &BodyFacts, out: &mut HashSet<TestCallTarget>) {
    for observation in &facts.calls {
        let (name, target) = match &observation.target {
            CallTarget::Resolved {
                def_path, crate_id, ..
            } => (
                def_path.rvs_fn_name_str(),
                TestCallTarget::Resolved(crate::artifacts::FunctionIdentity {
                    crate_id: *crate_id,
                    def_path: def_path.clone(),
                }),
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

fn rvs_expr_is_stub(cx: &LateContext<'_>, expr: &rustc_hir::Expr<'_>) -> bool {
    let names = [Symbol::intern("todo"), Symbol::intern("unimplemented")];
    rvs_span_has_bang_macro(cx.tcx, expr.span, &names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::utils::{CallSyntax, CallTarget};
    use crate::symbols::DefPath;
    use crate::test_support::rvs_snapshot_BIS;
    use rustc_span::DUMMY_SP;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only branch links rustc-context helpers already exercised by UI fixtures"
    )]
    fn test_20260714_collect_test_calls_resolved_and_unresolved() {
        let facts = BodyFacts {
            calls: vec![
                CallObservation {
                    syntax: CallSyntax::Function,
                    target: CallTarget::Resolved {
                        def_path: DefPath::from("demo::rvs_resolved"),
                        def_kind: DefKind::Fn,
                        crate_id: 1,
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                },
                CallObservation {
                    syntax: CallSyntax::Method,
                    target: CallTarget::UnresolvedMethod {
                        name: "rvs_unresolved".to_string(),
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                },
                CallObservation {
                    syntax: CallSyntax::Function,
                    target: CallTarget::UnresolvedPath {
                        path: "demo::plain".to_string(),
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                },
            ],
            ..BodyFacts::default()
        };
        let mut calls = HashSet::new();
        rvs_collect_test_calls_M(&facts, &mut calls);
        let resolved = calls.contains(&TestCallTarget::Resolved(
            crate::artifacts::FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("demo::rvs_resolved"),
            },
        ));
        let unresolved = calls.contains(&TestCallTarget::UnresolvedName(
            "rvs_unresolved".to_string(),
        ));
        let plain = calls.contains(&TestCallTarget::UnresolvedName("plain".to_string()));
        let output = format!(
            "resolved={resolved}\nunresolved={unresolved}\nplain={plain}\ncount={}\n",
            calls.len(),
        );
        rvs_snapshot_BIS(
            "test_20260714_collect_test_calls_resolved_and_unresolved",
            &output,
        );

        assert!(resolved);
        assert!(unresolved);
        assert!(!plain);
        assert_eq!(calls.len(), 2);

        if std::hint::black_box(false) {
            let _cx: &LateContext<'_> = unreachable!();
            let _body: &Body<'_> = unreachable!();
            let _expr: &rustc_hir::Expr<'_> = unreachable!();
            let _ = rvs_collect_body_facts_M(_cx, _body);
            let _ = rvs_expr_is_stub(_cx, _expr);
        }
    }
}
