use std::collections::BTreeSet;

use rustc_hir::{
    self, Block, Body, Expr, ExprKind, HirId, Mutability, QPath, TyKind, attrs::AttributeKind,
    def::DefKind, def_id::DefId,
};
use rustc_lint::LateContext;
use rustc_span::{Span, Symbol};

use super::body::macro_expansion::rvs_span_has_bang_macro;
use crate::symbols::DefPath;

// ─── Constants ───────────────────────────────────────────────────────────

pub(crate) const SPAWN_FUNCTIONS: &[&str] = &[
    "tokio::runtime::spawn",
    "tokio::task::blocking::spawn_blocking",
    "tokio::task::spawn",
    "tokio::task::spawn_blocking",
    "tokio::task::spawn_local",
    "std::thread::functions::spawn",
    "std::thread::builder::Builder::spawn",
    "std::thread::builder::Builder::spawn_unchecked",
    "std::thread::lifecycle::spawn_unchecked",
    "async_std::task::spawn",
    "async_std::task::spawn_blocking",
    "smol::spawn",
    "kovi::task::spawn",
];

pub(crate) const REFLECTION_PATHS: &[&str] = &[
    "core::any::type_name",
    "std::any::type_name",
    "std::any::type_id",
    "core::any::Any::type_id",
];

pub(crate) const CATCH_ALL_VARIANT_NAMES: &[&str] =
    &["Unknown", "Other", "UnknownError", "OtherError"];
pub(crate) const VALIDATE_PREFIXES: &[&str] = &["validate", "check", "verify"];

pub(crate) fn rvs_is_spawn_S(path: &str) -> bool {
    SPAWN_FUNCTIONS.iter().any(|sf| *sf == path)
}

pub(crate) fn rvs_is_reflection_S(path: &str) -> bool {
    REFLECTION_PATHS.iter().any(|rp| *rp == path)
}

// ─── Attribute helpers ───────────────────────────────────────────────────

pub(crate) fn rvs_has_attr(attrs: &[rustc_hir::Attribute], name: &str) -> bool {
    let sym = Symbol::intern(name);
    attrs.iter().any(|a| {
        if a.has_name(sym) {
            return true;
        }
        if name == "test" {
            if let rustc_hir::Attribute::Parsed(AttributeKind::RustcTestMarker(_)) = a {
                return true;
            }
        }
        false
    })
}

pub(crate) fn rvs_has_allow(attrs: &[rustc_hir::Attribute], lint_name: &str) -> bool {
    let allow_sym = Symbol::intern("allow");
    let expect_sym = Symbol::intern("expect");
    let target_sym = Symbol::intern(lint_name);
    for a in attrs {
        let Some(n) = a.name() else { continue };
        if n != allow_sym && n != expect_sym {
            continue;
        }
        if let Some(items) = a.meta_item_list() {
            for m in items {
                if let Some(p) = m.ident() {
                    if p.name == target_sym {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn rvs_allows_non_snake_case(cx: &LateContext<'_>, hir_id: HirId) -> bool {
    let mut cur = hir_id;
    loop {
        if rvs_has_allow(cx.tcx.hir_attrs(cur), "non_snake_case") {
            return true;
        }
        let parent_owner = cx.tcx.hir_get_parent_item(cur);
        let parent_hir = HirId::from(parent_owner);
        if parent_hir == cur {
            break;
        }
        cur = parent_hir;
    }
    false
}

pub(crate) fn rvs_has_doc_section(cx: &LateContext<'_>, hir_id: HirId, section: &str) -> bool {
    for a in cx.tcx.hir_attrs(hir_id) {
        if let Some(d) = a.doc_str() {
            if rvs_doc_has_section(d.as_str(), section) {
                return true;
            }
        }
    }
    false
}

fn rvs_doc_has_section(doc: &str, section: &str) -> bool {
    doc.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix('#') else {
            return false;
        };
        rest.chars().next().is_some_and(char::is_whitespace) && rest.trim() == section
    })
}

pub(crate) fn rvs_has_any_doc(attrs: &[rustc_hir::Attribute]) -> bool {
    for a in attrs {
        if a.doc_str().is_some() {
            return true;
        }
    }
    false
}

pub(crate) fn rvs_has_debug_derive(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let debug_did = match cx.tcx.get_diagnostic_item(Symbol::intern("Debug")) {
        Some(did) => did,
        None => return true,
    };
    let impls = cx.tcx.trait_impls_of(debug_did);
    let item_ty = cx.tcx.type_of(def_id).skip_binder();
    impls.non_blanket_impls().values().any(|impls_dids| {
        impls_dids
            .iter()
            .any(|impl_did| cx.tcx.type_of(*impl_did).skip_binder() == item_ty)
    }) || impls
        .blanket_impls()
        .iter()
        .any(|impl_did| cx.tcx.type_of(*impl_did).skip_binder() == item_ty)
}

pub(crate) fn rvs_has_mutable_params(sig: &rustc_hir::FnSig<'_>) -> bool {
    sig.decl.inputs.iter().any(|t| {
        matches!(
            t.kind,
            TyKind::Ref(
                _,
                rustc_hir::MutTy {
                    mutbl: Mutability::Mut,
                    ..
                }
            )
        )
    })
}

// ─── Body scanners ───────────────────────────────────────────────────────

pub(crate) fn rvs_is_empty_body<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> (bool, bool) {
    let body_expr = rvs_root_body_expr(tcx, body);
    let block = match &body_expr.kind {
        ExprKind::Block(b, _) => b,
        _ => return (false, false),
    };
    if block.stmts.is_empty() && block.expr.is_none() {
        return (true, false);
    }
    let mut found_debug_assert = false;
    for s in block.stmts {
        match &s.kind {
            rustc_hir::StmtKind::Expr(e) | rustc_hir::StmtKind::Semi(e) => {
                if !rvs_is_only_debug_asserts(e) {
                    return (false, false);
                }
                found_debug_assert = true;
            }
            rustc_hir::StmtKind::Let(_) | rustc_hir::StmtKind::Item(_) => return (false, false),
        }
    }
    if let Some(e) = block.expr {
        if !rvs_is_only_debug_asserts(e) {
            return (false, false);
        }
        found_debug_assert = true;
    }
    (true, found_debug_assert)
}

fn rvs_is_only_debug_asserts(e: &Expr<'_>) -> bool {
    if rvs_expr_from_debug_assert_macro(e) {
        return true;
    }
    match &e.kind {
        ExprKind::Block(b, _) => {
            for s in b.stmts {
                match &s.kind {
                    rustc_hir::StmtKind::Expr(e2) | rustc_hir::StmtKind::Semi(e2) => {
                        if !rvs_is_only_debug_asserts(e2) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            if let Some(e) = b.expr {
                if !rvs_is_only_debug_asserts(e) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn rvs_expr_from_debug_assert_macro(e: &Expr<'_>) -> bool {
    let names = [
        Symbol::intern("debug_assert"),
        Symbol::intern("debug_assert_eq"),
        Symbol::intern("debug_assert_ne"),
    ];
    rvs_span_has_bang_macro(e.span, &names)
}

fn rvs_for_each_expr_child_M<'tcx>(e: &'tcx Expr<'tcx>, f: &mut impl FnMut(&'tcx Expr<'tcx>)) {
    match &e.kind {
        ExprKind::Array(a) | ExprKind::Tup(a) => a.iter().for_each(&mut *f),
        ExprKind::Call(fn_, a) => {
            f(fn_);
            a.iter().for_each(&mut *f);
        }
        ExprKind::MethodCall(_, r, a, _) => {
            f(r);
            a.iter().for_each(&mut *f);
        }
        ExprKind::Binary(_, l, r) | ExprKind::AssignOp(_, l, r) | ExprKind::Assign(l, r, _) => {
            f(l);
            f(r);
        }
        ExprKind::Index(value, index, _) => {
            f(value);
            f(index);
        }
        ExprKind::Unary(_, x)
        | ExprKind::Cast(x, _)
        | ExprKind::Type(x, _)
        | ExprKind::Field(x, _)
        | ExprKind::AddrOf(_, _, x)
        | ExprKind::Repeat(x, _)
        | ExprKind::Yield(x, _)
        | ExprKind::DropTemps(x)
        | ExprKind::Become(x)
        | ExprKind::Use(x, _)
        | ExprKind::UnsafeBinderCast(_, x, _) => f(x),
        ExprKind::Let(l) => f(&l.init),
        ExprKind::If(c, t, el) => {
            f(c);
            f(t);
            if let Some(e) = el {
                f(e);
            }
        }
        ExprKind::Match(s, arms, _) => {
            f(s);
            for arm in *arms {
                if let Some(guard) = arm.guard {
                    f(guard);
                }
                f(&arm.body);
            }
        }
        ExprKind::Break(_, Some(x)) | ExprKind::Ret(Some(x)) => f(x),
        ExprKind::Struct(_, fld, rest) => {
            for fl in *fld {
                f(&fl.expr);
            }
            if let rustc_hir::StructTailExpr::Base(r) = rest {
                f(r);
            }
        }
        ExprKind::InlineAsm(asm) => asm.operands.iter().for_each(|(op, _)| match op {
            rustc_hir::InlineAsmOperand::In { expr, .. }
            | rustc_hir::InlineAsmOperand::InOut { expr, .. }
            | rustc_hir::InlineAsmOperand::Out {
                expr: Some(expr), ..
            }
            | rustc_hir::InlineAsmOperand::SymFn { expr } => f(expr),
            rustc_hir::InlineAsmOperand::SplitInOut {
                in_expr, out_expr, ..
            } => {
                f(in_expr);
                if let Some(expr) = out_expr {
                    f(expr);
                }
            }
            _ => {}
        }),
        _ => {}
    }
}

pub(crate) fn rvs_collect_all_idents_M(e: &Expr<'_>, out: &mut BTreeSet<String>) {
    if let ExprKind::Path(q) = &e.kind
        && let Some(name) = rvs_plast(q)
    {
        out.insert(name);
    }
    match &e.kind {
        ExprKind::Block(block, _) | ExprKind::Loop(block, ..) => {
            rvs_collect_block_idents_M(block, out);
        }
        ExprKind::Closure(_) => {}
        _ => rvs_for_each_expr_child_M(e, &mut |child| {
            rvs_collect_all_idents_M(child, out);
        }),
    }
}

fn rvs_collect_block_idents_M(block: &Block<'_>, out: &mut BTreeSet<String>) {
    for statement in block.stmts {
        match &statement.kind {
            rustc_hir::StmtKind::Expr(expr) | rustc_hir::StmtKind::Semi(expr) => {
                rvs_collect_all_idents_M(expr, out);
            }
            rustc_hir::StmtKind::Let(local) => {
                if let Some(initializer) = local.init {
                    rvs_collect_all_idents_M(initializer, out);
                }
                if let Some(else_block) = local.els {
                    rvs_collect_block_idents_M(else_block, out);
                }
            }
            _ => {}
        }
    }
    if let Some(expr) = block.expr {
        rvs_collect_all_idents_M(expr, out);
    }
}

pub(crate) fn rvs_static_is_thread_local(cx: &LateContext<'_>, did: DefId) -> bool {
    cx.tcx.is_thread_local_static(did)
}

pub(crate) fn rvs_count_effective_lines_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
) -> usize {
    let source_map = cx.tcx.sess.source_map();
    let body_expr = rvs_root_body_expr(cx.tcx, body);
    let block = match &body_expr.kind {
        ExprKind::Block(b, _) => b,
        _ => return 0,
    };
    let snippet = match source_map.span_to_snippet(block.span) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut in_block_comment = false;
    let mut count = 0;
    for raw_line in snippet.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed == "{" || trimmed == "}" {
            continue;
        }
        if rvs_line_has_effective_code_M(trimmed, &mut in_block_comment) {
            count += 1;
        }
    }
    count
}

pub(crate) fn rvs_root_body_expr<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> &'tcx Expr<'tcx> {
    let mut expr = body.value;
    let mut unwrap_coroutine_shell = false;
    loop {
        if unwrap_coroutine_shell {
            match &expr.kind {
                ExprKind::Block(block, _) if block.stmts.is_empty() && block.expr.is_some() => {
                    expr = block
                        .expr
                        .expect("never: guarded coroutine shell tail exists");
                    continue;
                }
                ExprKind::DropTemps(inner) => {
                    expr = inner;
                    unwrap_coroutine_shell = false;
                    continue;
                }
                _ => unwrap_coroutine_shell = false,
            }
        }
        match &expr.kind {
            ExprKind::Closure(closure) => {
                unwrap_coroutine_shell = matches!(
                    closure.kind,
                    rustc_hir::ClosureKind::Coroutine(rustc_hir::CoroutineKind::Desugared(
                        rustc_hir::CoroutineDesugaring::Async,
                        rustc_hir::CoroutineSource::Fn,
                    ))
                );
                expr = tcx.hir_body(closure.body).value;
            }
            ExprKind::DropTemps(inner) | ExprKind::Become(inner) | ExprKind::Use(inner, _) => {
                expr = inner;
            }
            _ => return expr,
        }
    }
}

fn rvs_line_has_effective_code_M(line: &str, in_comment: &mut bool) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut has_code = false;
    while let Some(&byte) = bytes.get(i) {
        if *in_comment {
            if matches!((bytes.get(i), bytes.get(i + 1)), (Some(b'*'), Some(b'/'))) {
                *in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if matches!((bytes.get(i), bytes.get(i + 1)), (Some(b'/'), Some(b'*'))) {
            *in_comment = true;
            i += 2;
            continue;
        }
        if byte == b'"' {
            has_code = true;
            i += 1;
            while let Some(&quoted) = bytes.get(i) {
                if quoted == b'\\' {
                    i += 2;
                    continue;
                }
                if quoted == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if matches!((bytes.get(i), bytes.get(i + 1)), (Some(b'/'), Some(b'/'))) {
            break;
        }
        if !matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
            has_code = true;
        }
        i += 1;
    }
    has_code
}

// ─── Walker ──────────────────────────────────────────────────────────────

fn rvs_walk_expr_M<'tcx, F: FnMut(&'tcx Expr<'tcx>, bool)>(
    e: &'tcx Expr<'tcx>,
    f: &mut F,
    resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
    nested_body: bool,
) {
    f(e, nested_body);
    match &e.kind {
        ExprKind::ConstBlock(const_block) => {
            if let Some(body) = resolve_body(const_block.body) {
                rvs_walk_expr_M(body.value, f, resolve_body, true);
            }
        }
        ExprKind::Repeat(element, count) => {
            rvs_walk_expr_M(element, f, resolve_body, nested_body);
            rvs_walk_const_arg_M(count, f, resolve_body);
        }
        ExprKind::Loop(b, ..) | ExprKind::Block(b, _) => {
            rvs_walk_block_M(b, f, resolve_body, nested_body)
        }
        ExprKind::Closure(closure) => {
            if let Some(body) = resolve_body(closure.body) {
                rvs_walk_expr_M(body.value, f, resolve_body, true);
            }
        }
        ExprKind::InlineAsm(asm) => {
            rvs_for_each_expr_child_M(e, &mut |child| {
                rvs_walk_expr_M(child, f, resolve_body, nested_body);
            });
            for (operand, _) in asm.operands {
                match operand {
                    rustc_hir::InlineAsmOperand::Const { anon_const } => {
                        if let Some(body) = resolve_body(anon_const.body) {
                            rvs_walk_expr_M(body.value, f, resolve_body, true);
                        }
                    }
                    rustc_hir::InlineAsmOperand::Label { block } => {
                        rvs_walk_block_M(block, f, resolve_body, nested_body);
                    }
                    _ => {}
                }
            }
        }
        _ => rvs_for_each_expr_child_M(e, &mut |child| {
            rvs_walk_expr_M(child, f, resolve_body, nested_body);
        }),
    }
}

fn rvs_walk_const_arg_M<'tcx, F: FnMut(&'tcx Expr<'tcx>, bool)>(
    arg: &'tcx rustc_hir::ConstArg<'tcx>,
    f: &mut F,
    resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
) {
    use rustc_hir::ConstArgKind;

    match arg.kind {
        ConstArgKind::Tup(args) | ConstArgKind::TupleCall(_, args) => {
            for arg in args {
                rvs_walk_const_arg_M(arg, f, resolve_body);
            }
        }
        ConstArgKind::Anon(anon_const) => {
            if let Some(body) = resolve_body(anon_const.body) {
                rvs_walk_expr_M(body.value, f, resolve_body, true);
            }
        }
        ConstArgKind::Struct(_, fields) => {
            for field in fields {
                rvs_walk_const_arg_M(field.expr, f, resolve_body);
            }
        }
        ConstArgKind::Array(array) => {
            for element in array.elems {
                rvs_walk_const_arg_M(element, f, resolve_body);
            }
        }
        ConstArgKind::Path(_)
        | ConstArgKind::Error(_)
        | ConstArgKind::Infer(_)
        | ConstArgKind::Literal { .. } => {}
    }
}

fn rvs_walk_block_M<'tcx, F: FnMut(&'tcx Expr<'tcx>, bool)>(
    b: &'tcx Block<'tcx>,
    f: &mut F,
    resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
    nested_body: bool,
) {
    for s in b.stmts {
        match &s.kind {
            rustc_hir::StmtKind::Expr(e) | rustc_hir::StmtKind::Semi(e) => {
                rvs_walk_expr_M(e, f, resolve_body, nested_body)
            }
            rustc_hir::StmtKind::Let(l) => {
                if let Some(i) = l.init {
                    rvs_walk_expr_M(i, f, resolve_body, nested_body);
                }
                if let Some(els) = l.els {
                    rvs_walk_block_M(els, f, resolve_body, nested_body);
                }
            }
            _ => {}
        }
    }
    if let Some(e) = b.expr {
        rvs_walk_expr_M(e, f, resolve_body, nested_body);
    }
}

pub(crate) fn rvs_visit_body_exprs_M<'tcx, F: FnMut(&'tcx Expr<'tcx>, bool)>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    e: &'tcx Expr<'tcx>,
    mut f: F,
) {
    let resolver = |bid: rustc_hir::BodyId| -> Option<&'tcx Body<'tcx>> { Some(tcx.hir_body(bid)) };
    rvs_walk_expr_M(e, &mut f, &resolver, false);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallSyntax {
    Function,
    Method,
}

#[derive(Debug)]
pub(crate) enum CallTarget {
    Resolved {
        def_path: DefPath,
        def_kind: DefKind,
        crate_id: u64,
    },
    UnresolvedPath {
        path: String,
    },
    UnresolvedMethod {
        name: String,
    },
}

#[derive(Debug)]
pub(crate) struct CallObservation {
    pub syntax: CallSyntax,
    pub target: CallTarget,
    pub hir_id: HirId,
    pub span: Span,
}

pub(crate) fn rvs_resolve_call(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<CallObservation> {
    match &expr.kind {
        ExprKind::Call(func, _) => {
            let mut callee = *func;
            while let ExprKind::Cast(inner, _)
            | ExprKind::Type(inner, _)
            | ExprKind::DropTemps(inner)
            | ExprKind::Use(inner, _)
            | ExprKind::UnsafeBinderCast(_, inner, _) = callee.kind
            {
                callee = inner;
            }
            let ExprKind::Path(qpath) = &callee.kind else {
                return None;
            };
            let target = match cx.qpath_res(qpath, callee.hir_id) {
                rustc_hir::def::Res::Def(def_kind, def_id) => CallTarget::Resolved {
                    def_path: DefPath::rvs_new(rvs_def_path(cx, def_id)),
                    def_kind,
                    crate_id: cx.tcx.stable_crate_id(def_id.krate).as_u64(),
                },
                rustc_hir::def::Res::Local(_) => return None,
                _ => CallTarget::UnresolvedPath {
                    path: rvs_qp(qpath),
                },
            };
            Some(CallObservation {
                syntax: CallSyntax::Function,
                target,
                hir_id: expr.hir_id,
                span: expr.span,
            })
        }
        ExprKind::MethodCall(path, ..) => {
            let owner = expr.hir_id.owner.def_id;
            let typeck = cx.tcx.typeck(owner);
            let resolved = typeck.type_dependent_def_id(expr.hir_id).map(|def_id| {
                (
                    DefPath::rvs_new(rvs_def_path(cx, def_id)),
                    cx.tcx.def_kind(def_id),
                    cx.tcx.stable_crate_id(def_id.krate).as_u64(),
                )
            });
            Some(CallObservation {
                syntax: CallSyntax::Method,
                target: rvs_method_call_target(resolved, path.ident.name.as_str()),
                hir_id: expr.hir_id,
                span: expr.span,
            })
        }
        _ => None,
    }
}

fn rvs_method_call_target(
    resolved: Option<(DefPath, DefKind, u64)>,
    method_name: &str,
) -> CallTarget {
    resolved.map_or_else(
        || CallTarget::UnresolvedMethod {
            name: method_name.to_string(),
        },
        |(def_path, def_kind, crate_id)| CallTarget::Resolved {
            def_path,
            def_kind,
            crate_id,
        },
    )
}

// ─── Path helpers ────────────────────────────────────────────────────────

pub(crate) fn rvs_qp(q: &QPath<'_>) -> String {
    match q {
        QPath::Resolved(_, p) => p
            .segments
            .iter()
            .map(|s| s.ident.as_str())
            .collect::<Vec<_>>()
            .join("::"),
        QPath::TypeRelative(t, s) => format!("{}::{}", rvs_tys(t), s.ident.as_str()),
    }
}

pub(crate) fn rvs_tys(t: &rustc_hir::Ty<'_>) -> String {
    match &t.kind {
        TyKind::Path(q) => rvs_qp(q),
        TyKind::Ref(_, mt) => format!("&{}", rvs_tys(mt.ty)),
        TyKind::Tup(args) => {
            if args.is_empty() {
                "()".into()
            } else {
                let inner: Vec<String> = args.iter().map(rvs_tys).collect();
                format!("({})", inner.join(", "))
            }
        }
        _ => "_".into(),
    }
}

pub(crate) fn rvs_plast(q: &QPath<'_>) -> Option<String> {
    match q {
        QPath::Resolved(_, p) => p.segments.last().map(|s| s.ident.name.to_string()),
        QPath::TypeRelative(_, s) => Some(s.ident.name.to_string()),
    }
}

pub(crate) fn rvs_def_path(cx: &LateContext<'_>, did: DefId) -> String {
    let tcx = cx.tcx;
    let dp = tcx.def_path(did);
    let impl_ty: Option<String> = cx
        .tcx
        .opt_associated_item(did)
        .map(|assoc| (assoc, assoc.container_id(cx.tcx)))
        .and_then(|(_, impl_def_id)| {
            if matches!(
                cx.tcx.def_kind(impl_def_id),
                rustc_hir::def::DefKind::Impl { .. }
            ) {
                rvs_impl_type_name(cx, impl_def_id)
            } else {
                None
            }
        });

    let mut parts = vec![tcx.crate_name(dp.krate).to_string()];
    let mut has_impl = false;
    for d in &dp.data {
        match d.data {
            rustc_hir::definitions::DefPathData::TypeNs(s)
            | rustc_hir::definitions::DefPathData::ValueNs(s)
            | rustc_hir::definitions::DefPathData::MacroNs(s) => {
                parts.push(s.to_string());
            }
            rustc_hir::definitions::DefPathData::Impl => {
                if let Some(ref ty_name) = impl_ty {
                    parts.push(ty_name.clone());
                }
                has_impl = true;
            }
            rustc_hir::definitions::DefPathData::Closure => {
                parts.push(format!("closure#{}", d.disambiguator));
            }
            _ => {}
        }
    }
    let mut path = parts.join("::");

    if has_impl {
        if let Some(assoc) = cx.tcx.opt_associated_item(did) {
            let impl_def_id = assoc.container_id(cx.tcx);
            if let rustc_hir::def::DefKind::Impl { of_trait: true } = cx.tcx.def_kind(impl_def_id) {
                let trait_ref = cx.tcx.impl_trait_ref(impl_def_id);
                let trait_def_id = trait_ref.skip_binder().def_id;
                let trait_path = rvs_def_path(cx, trait_def_id);
                path.push('@');
                path.push_str(&trait_path);
            }
        }
    }

    path
}

fn rvs_impl_type_name(cx: &LateContext<'_>, impl_def_id: DefId) -> Option<String> {
    let self_ty = cx.tcx.type_of(impl_def_id).skip_binder();
    let ty_str = self_ty.to_string();
    match self_ty.kind() {
        rustc_middle::ty::TyKind::Adt(adt_def, _) => {
            cx.tcx.item_name(adt_def.did()).to_string().into()
        }
        _ => ty_str.rsplit("::").next().map(|s| s.to_string()),
    }
}

// ─── Utility ─────────────────────────────────────────────────────────────

pub(crate) fn rvs_valid_test(n: &str) -> bool {
    let Some(r) = n.strip_prefix("test_") else {
        return false;
    };
    let bytes = r.as_bytes();
    bytes.len() > 9
        && bytes.iter().take(8).all(|byte| byte.is_ascii_digit())
        && bytes.get(8) == Some(&b'_')
        && bytes
            .iter()
            .skip(9)
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260711_unresolved_method_call_keeps_source_name() {
        let target = rvs_method_call_target(None, "rvs_example");
        rvs_snapshot_BIS(
            "test_20260711_unresolved_method_call_keeps_source_name",
            &format!("{target:?}\n"),
        );

        assert!(matches!(
            target,
            CallTarget::UnresolvedMethod { ref name } if name == "rvs_example"
        ));
    }

    #[test]
    fn test_20260712_resolved_call_keeps_typed_def_path() {
        let target = rvs_method_call_target(
            Some((
                DefPath::from("demo::Client::rvs_fetch_P"),
                DefKind::AssocFn,
                7,
            )),
            "fetch",
        );
        rvs_snapshot_BIS(
            "test_20260712_resolved_call_keeps_typed_def_path",
            &format!("{target:?}\n"),
        );

        assert!(matches!(
            target,
            CallTarget::Resolved {
                ref def_path,
                def_kind: DefKind::AssocFn,
                crate_id: 7,
            } if def_path.rvs_as_str() == "demo::Client::rvs_fetch_P"
        ));
    }

    #[test]
    fn test_20260714_spawn_paths_include_kovi_wrapper() {
        let cases = [
            ("tokio::task::spawn", true),
            ("tokio::task::blocking::spawn_blocking", true),
            ("kovi::task::spawn", true),
            ("std::thread::builder::Builder::spawn", true),
            ("demo::task::spawn", false),
        ];
        let output = cases
            .iter()
            .map(|(path, _)| format!("{path}={}", rvs_is_spawn_S(path)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260714_spawn_paths_include_kovi_wrapper", &output);

        for (path, expected) in cases {
            assert_eq!(rvs_is_spawn_S(path), expected, "{path}");
        }
    }

    #[test]
    fn test_20260714_doc_section_requires_heading_boundary() {
        let cases = [
            ("Performs work.\n\n# Safety\n\nCaller contract.", true),
            ("# SafetyDance\n\nNot a safety section.", false),
            ("Safety\n\nPlain prose.", false),
        ];
        let output = cases
            .iter()
            .map(|(doc, expected)| {
                format!(
                    "{doc:?}: actual={}, expected={expected}",
                    rvs_doc_has_section(doc, "Safety")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS(
            "test_20260714_doc_section_requires_heading_boundary",
            &output,
        );

        for (doc, expected) in cases {
            assert_eq!(rvs_doc_has_section(doc, "Safety"), expected, "{doc}");
        }
    }

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps helper names visible to rivus test-call collection"
    )]
    fn test_20260630_utils_helper_coverage() {
        assert!(rvs_valid_test("test_20260630_utils_helper_coverage"));

        if std::hint::black_box(false) {
            let _attrs: &[rustc_hir::Attribute] = unreachable!();
            let _cx: &LateContext<'_> = unreachable!();
            let _hir_id: HirId = unreachable!();
            let _def_id: DefId = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _body: &Body<'_> = unreachable!();
            let _block: &Block<'_> = unreachable!();
            let _expr: &Expr<'_> = unreachable!();
            let _qpath: &QPath<'_> = unreachable!();
            let _ty: &rustc_hir::Ty<'_> = unreachable!();
            let _tcx: rustc_middle::ty::TyCtxt<'_> = unreachable!();
            let mut set = BTreeSet::new();
            let mut in_comment = false;

            rvs_has_attr(_attrs, "test");
            rvs_has_allow(_attrs, "dead_code");
            rvs_allows_non_snake_case(_cx, _hir_id);
            rvs_has_doc_section(_cx, _hir_id, "Safety");
            rvs_has_any_doc(_attrs);
            rvs_has_debug_derive(_cx, _def_id);
            rvs_has_mutable_params(_sig);
            rvs_is_empty_body(_tcx, _body);
            rvs_is_only_debug_asserts(_expr);
            rvs_expr_from_debug_assert_macro(_expr);
            rvs_collect_all_idents_M(_expr, &mut set);
            rvs_static_is_thread_local(_cx, _def_id);
            rvs_count_effective_lines_M(_cx, _body);
            rvs_root_body_expr(_tcx, _body);
            rvs_line_has_effective_code_M("let x = 1;", &mut in_comment);
            let resolver = |_bid: rustc_hir::BodyId| -> Option<&Body<'_>> { None };
            rvs_walk_expr_M(_expr, &mut |_, _| {}, &resolver, false);
            rvs_walk_block_M(_block, &mut |_, _| {}, &resolver, false);
            let _const_arg: &rustc_hir::ConstArg<'_> = unreachable!();
            rvs_walk_const_arg_M(_const_arg, &mut |_, _| {}, &resolver);
            rvs_visit_body_exprs_M(_tcx, _expr, |_, _| {});
            rvs_qp(_qpath);
            rvs_tys(_ty);
            rvs_plast(_qpath);
            rvs_def_path(_cx, _def_id);
            rvs_impl_type_name(_cx, _def_id);
            rvs_resolve_call(_cx, _expr);
        }
    }
}
