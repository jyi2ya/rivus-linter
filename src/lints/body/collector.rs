use std::collections::{HashMap, HashSet};

use rustc_hir::{
    Body, ExprKind, HirId, LocalSource, Mutability, PatKind, def::DefKind, def_id::DefId,
};
use rustc_lint::LateContext;
use rustc_middle::ty::{TyKind, TypingEnv};
use rustc_span::{Span, Symbol, sym};

use super::super::utils::{
    CallObservation, CallSyntax, CallTarget, ObservationKind, rvs_collect_local_bindings_M,
    rvs_def_id_is_fn_trait_operation, rvs_def_path, rvs_resolve_call, rvs_root_body_expr,
    rvs_static_is_thread_local, rvs_visit_body_exprs,
};
use super::macro_expansion::rvs_span_has_bang_macro;
use crate::lints::ctx::TestCallTarget;
use crate::symbols::DefPath;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImplicitExecutionKind {
    ExplicitFnTraitCall,
    InlineAsm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ImplicitExecutionSite {
    pub(crate) kind: ImplicitExecutionKind,
    pub(crate) hir_id: HirId,
    pub(crate) span: Span,
}

#[derive(Debug, Default)]
pub(crate) struct BodyFacts {
    pub(crate) call_observations: Vec<CallObservation>,
    pub(crate) has_static_ref: bool,
    pub(crate) has_static_mut_ref: bool,
    pub(crate) has_thread_local_ref: bool,
    pub(crate) has_stub: bool,
    pub(crate) debug_assert_bindings: HashSet<HirId>,
    pub(crate) result_swallow_calls: Vec<(HirId, Span, CallSyntax, String)>,
    pub(crate) result_drop_calls: Vec<(HirId, Span)>,
    pub(crate) unsupported_implicit_execution: Vec<ImplicitExecutionSite>,
}

pub(crate) fn rvs_collect_body_facts<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
    collect_lint_facts: bool,
) -> BodyFacts {
    let mut facts = BodyFacts::default();
    let debug_assert_macros = [
        Symbol::intern("debug_assert"),
        Symbol::intern("debug_assert_eq"),
        Symbol::intern("debug_assert_ne"),
    ];
    let root_expr = rvs_root_body_expr(cx.tcx, body);
    let root_owner = root_expr.hir_id.owner.def_id;
    let async_param_aliases = if collect_lint_facts {
        rvs_async_param_aliases(cx, root_expr)
    } else {
        HashMap::new()
    };
    let mut direct_callee_hir_ids: HashSet<HirId> = HashSet::new();
    let mut coverage_registered_hir_ids: HashSet<HirId> = HashSet::new();

    rvs_visit_body_exprs(cx.tcx, root_expr, |expr, body_owner| {
        rvs_collect_static_facts_M(cx, expr, &mut facts);

        if collect_lint_facts && let Some(kind) = rvs_implicit_execution_kind(cx, expr) {
            facts
                .unsupported_implicit_execution
                .push(ImplicitExecutionSite {
                    kind,
                    hir_id: expr.hir_id,
                    span: expr.span,
                });
        }

        if matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
            if let Some(mut observation) = rvs_resolve_call(cx, expr) {
                observation.body_owner = body_owner;
                if collect_lint_facts && observation.kind == ObservationKind::Direct {
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
                }
                if let Some(callee_hir_id) = rvs_direct_callee_hir_id(expr) {
                    direct_callee_hir_ids.insert(callee_hir_id);
                }
                facts.call_observations.push(observation);
            }

            for (target, hir_id, span) in rvs_test_coverage_targets(cx, expr) {
                coverage_registered_hir_ids.insert(hir_id);
                facts.call_observations.push(CallObservation {
                    kind: ObservationKind::Direct,
                    syntax: CallSyntax::Function,
                    target: rvs_resolved_target(cx, target),
                    hir_id,
                    span,
                    body_owner,
                });
            }
        }

        rvs_collect_function_ref_M(
            cx,
            expr,
            body_owner,
            &direct_callee_hir_ids,
            &coverage_registered_hir_ids,
            &mut facts,
        );

        if collect_lint_facts && !facts.has_stub && rvs_expr_is_stub(cx, expr) {
            facts.has_stub = true;
        }
        if collect_lint_facts
            && body_owner == root_owner
            && expr.span.from_expansion()
            && rvs_span_has_bang_macro(cx.tcx, expr.span, debug_assert_macros.as_slice())
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

fn rvs_implicit_execution_kind(
    cx: &LateContext<'_>,
    expr: &rustc_hir::Expr<'_>,
) -> Option<ImplicitExecutionKind> {
    match expr.kind {
        ExprKind::InlineAsm(_) => Some(ImplicitExecutionKind::InlineAsm),
        ExprKind::Call(..) | ExprKind::MethodCall(..)
            if rvs_explicit_call_def_id(cx, expr)
                .is_some_and(|def_id| rvs_def_id_is_fn_trait_operation(cx, def_id)) =>
        {
            Some(ImplicitExecutionKind::ExplicitFnTraitCall)
        }
        _ => None,
    }
}

fn rvs_explicit_call_def_id(cx: &LateContext<'_>, expr: &rustc_hir::Expr<'_>) -> Option<DefId> {
    match expr.kind {
        ExprKind::Call(callee, _) => {
            let mut callee = callee;
            loop {
                match callee.kind {
                    ExprKind::Cast(inner, _)
                    | ExprKind::Type(inner, _)
                    | ExprKind::DropTemps(inner)
                    | ExprKind::Use(inner, _)
                    | ExprKind::UnsafeBinderCast(_, inner, _) => callee = inner,
                    ExprKind::Block(block, _) => callee = block.expr?,
                    _ => break,
                }
            }
            let ExprKind::Path(qpath) = &callee.kind else {
                return None;
            };
            let rustc_hir::def::Res::Def(_, def_id) = cx.qpath_res(qpath, callee.hir_id) else {
                return None;
            };
            Some(def_id)
        }
        ExprKind::MethodCall(..) => cx
            .tcx
            .typeck(expr.hir_id.owner.def_id)
            .type_dependent_def_id(expr.hir_id),
        _ => None,
    }
}

fn rvs_collect_static_facts_M(
    cx: &LateContext<'_>,
    expr: &rustc_hir::Expr<'_>,
    facts: &mut BodyFacts,
) {
    match &expr.kind {
        ExprKind::Path(qpath) => {
            if let rustc_hir::def::Res::Def(DefKind::Static { mutability, .. }, def_id) =
                cx.qpath_res(qpath, expr.hir_id)
            {
                rvs_record_static_facts_M(cx, def_id, mutability, facts);
            }
        }
        ExprKind::InlineAsm(asm) => {
            for (operand, _) in asm.operands {
                let rustc_hir::InlineAsmOperand::SymStatic { def_id, .. } = operand else {
                    continue;
                };
                if let DefKind::Static { mutability, .. } = cx.tcx.def_kind(*def_id) {
                    rvs_record_static_facts_M(cx, *def_id, mutability, facts);
                }
            }
        }
        _ => {}
    }
}

fn rvs_record_static_facts_M(
    cx: &LateContext<'_>,
    def_id: DefId,
    mutability: Mutability,
    facts: &mut BodyFacts,
) {
    if rvs_static_is_thread_local(cx, def_id) {
        facts.has_thread_local_ref = true;
    }
    match mutability {
        Mutability::Mut => facts.has_static_mut_ref = true,
        Mutability::Not => facts.has_static_ref = true,
    }
}

const fn rvs_direct_callee_hir_id(expr: &rustc_hir::Expr<'_>) -> Option<HirId> {
    if let ExprKind::Call(func, _) = expr.kind {
        let mut callee = func;
        loop {
            match callee.kind {
                ExprKind::Cast(inner, _)
                | ExprKind::Type(inner, _)
                | ExprKind::DropTemps(inner)
                | ExprKind::Use(inner, _)
                | ExprKind::UnsafeBinderCast(_, inner, _) => callee = inner,
                ExprKind::Block(block, _) => {
                    let Some(inner) = block.expr else {
                        return None;
                    };
                    callee = inner;
                }
                _ => break,
            }
        }
        if let ExprKind::Path(_) = callee.kind {
            return Some(callee.hir_id);
        }
    }
    None
}

fn rvs_collect_function_ref_M(
    cx: &LateContext<'_>,
    expr: &rustc_hir::Expr<'_>,
    body_owner: rustc_hir::def_id::LocalDefId,
    direct_callee_hir_ids: &HashSet<HirId>,
    coverage_registered_hir_ids: &HashSet<HirId>,
    facts: &mut BodyFacts,
) {
    let ExprKind::Path(qpath) = &expr.kind else {
        return;
    };
    if direct_callee_hir_ids.contains(&expr.hir_id) {
        return;
    }
    if coverage_registered_hir_ids.contains(&expr.hir_id) {
        return;
    }
    let rustc_hir::def::Res::Def(def_kind @ (DefKind::Fn | DefKind::AssocFn), def_id) =
        cx.qpath_res(qpath, expr.hir_id)
    else {
        return;
    };
    let crate_id = cx.tcx.stable_crate_id(def_id.krate).as_u64();
    facts.call_observations.push(CallObservation {
        kind: ObservationKind::FunctionReference,
        syntax: CallSyntax::Function,
        target: CallTarget::Resolved {
            def_path: DefPath::rvs_new(rvs_def_path(cx, def_id)),
            def_kind,
            crate_id,
        },
        hir_id: expr.hir_id,
        span: expr.span,
        body_owner,
    });
}

fn rvs_test_coverage_targets(
    cx: &LateContext<'_>,
    expr: &rustc_hir::Expr<'_>,
) -> Vec<(DefId, HirId, Span)> {
    let ExprKind::Call(callee, arguments) = expr.kind else {
        return Vec::new();
    };
    let ExprKind::Path(qpath) = &callee.kind else {
        return Vec::new();
    };
    let rustc_hir::def::Res::Def(DefKind::Fn, helper) = cx.qpath_res(qpath, callee.hir_id) else {
        return Vec::new();
    };
    if cx
        .tcx
        .get_diagnostic_item(Symbol::intern("rivus_test_coverage_registration"))
        != Some(helper)
    {
        return Vec::new();
    }

    let mut targets = Vec::new();
    for argument in arguments {
        rvs_collect_direct_fn_paths_M(cx, argument, &mut targets);
    }
    targets
}

/// Collect direct function-item paths from a coverage registration argument.
/// Accepts a bare `Fn`/`AssocFn` path or a tuple of such paths. Any other
/// expression (calls, casts, wrappers) is ignored.
fn rvs_collect_direct_fn_paths_M(
    cx: &LateContext<'_>,
    expr: &rustc_hir::Expr<'_>,
    out: &mut Vec<(DefId, HirId, Span)>,
) {
    match &expr.kind {
        ExprKind::Path(qpath) => {
            if let rustc_hir::def::Res::Def(DefKind::Fn | DefKind::AssocFn, def_id) =
                cx.qpath_res(qpath, expr.hir_id)
            {
                out.push((def_id, expr.hir_id, expr.span));
            }
        }
        ExprKind::Tup(elements) => {
            for element in *elements {
                rvs_collect_direct_fn_paths_M(cx, element, out);
            }
        }
        _ => {}
    }
}

fn rvs_resolved_target(cx: &LateContext<'_>, def_id: DefId) -> CallTarget {
    CallTarget::Resolved {
        def_path: crate::symbols::DefPath::rvs_new(super::super::utils::rvs_def_path(cx, def_id)),
        def_kind: cx.tcx.def_kind(def_id),
        crate_id: cx.tcx.stable_crate_id(def_id.krate).as_u64(),
    }
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
        .try_normalize_erasing_regions(TypingEnv::non_body_analysis(cx.tcx, owner), expr_type)
        .unwrap_or(expr_type);
    matches!(
        expr_type.kind(),
        TyKind::Adt(adt, _) if cx.tcx.is_diagnostic_item(sym::Result, adt.did())
    )
}

pub(crate) fn rvs_collect_test_calls_M(facts: &BodyFacts, out: &mut HashSet<TestCallTarget>) {
    for observation in &facts.call_observations {
        if observation.kind != ObservationKind::Direct {
            continue;
        }
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
                let name = rvs_unresolved_call_name(path);
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

/// Last path segment of an unresolved call path, used as the call's short name.
pub(crate) fn rvs_unresolved_call_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn rvs_expr_is_stub(cx: &LateContext<'_>, expr: &rustc_hir::Expr<'_>) -> bool {
    let names = [Symbol::intern("todo"), Symbol::intern("unimplemented")];
    rvs_span_has_bang_macro(cx.tcx, expr.span, names.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::utils::{CallSyntax, CallTarget, ObservationKind};
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};
    use rustc_span::DUMMY_SP;

    #[test]
    fn test_20260714_collect_test_calls_resolved_and_unresolved() {
        let facts = BodyFacts {
            call_observations: vec![
                CallObservation {
                    kind: ObservationKind::Direct,
                    syntax: CallSyntax::Function,
                    target: CallTarget::Resolved {
                        def_path: DefPath::from("demo::rvs_resolved"),
                        def_kind: DefKind::Fn,
                        crate_id: 1,
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                    body_owner: rustc_hir::CRATE_HIR_ID.owner.def_id,
                },
                CallObservation {
                    kind: ObservationKind::Direct,
                    syntax: CallSyntax::Method,
                    target: CallTarget::UnresolvedMethod {
                        name: "rvs_unresolved".to_string(),
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                    body_owner: rustc_hir::CRATE_HIR_ID.owner.def_id,
                },
                CallObservation {
                    kind: ObservationKind::Direct,
                    syntax: CallSyntax::Function,
                    target: CallTarget::UnresolvedPath {
                        path: "demo::plain".to_string(),
                    },
                    hir_id: rustc_hir::CRATE_HIR_ID,
                    span: DUMMY_SP,
                    body_owner: rustc_hir::CRATE_HIR_ID.owner.def_id,
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

        rvs_register_test_coverage((rvs_collect_body_facts, rvs_expr_is_stub));
    }
}
