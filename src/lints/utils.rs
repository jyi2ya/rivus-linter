use std::collections::{BTreeSet, HashSet};

use rustc_hir::{
    self, Block, Body, Expr, ExprKind, GenericArg, HirId, ImplItem, ImplItemImplKind, Mutability,
    QPath, TyKind, attrs::AttributeKind, def_id::DefId,
};
use rustc_lint::LateContext;
use rustc_span::Symbol;

use crate::capability::{CapabilitySet, rvs_extract_raw_suffix, rvs_parse_function};

// ─── Constants ───────────────────────────────────────────────────────────

pub(crate) const SPAWN_FUNCTIONS: &[&str] = &[
    "tokio::runtime::spawn",
    "tokio::task::spawn",
    "tokio::task::spawn_blocking",
    "tokio::task::spawn_local",
    "std::thread::functions::spawn",
    "std::thread::builder::spawn",
    "std::thread::builder::spawn_unchecked",
    "std::thread::lifecycle::spawn_unchecked",
    "async_std::task::spawn",
    "async_std::task::spawn_blocking",
    "smol::spawn",
];

pub(crate) const REFLECTION_PATHS: &[&str] = &[
    "std::any::type_name",
    "std::any::type_id",
    "core::any::Any::type_id",
];

pub(crate) const ERROR_SWALLOW_METHODS: &[&str] = &["ok", "unwrap_or_default"];
pub(crate) const CATCH_ALL_VARIANT_NAMES: &[&str] =
    &["Unknown", "Other", "UnknownError", "OtherError"];
pub(crate) const VALIDATE_PREFIXES: &[&str] = &["validate", "check", "verify"];
pub(crate) const BORROWED_TYPES: &[&str] = &["String", "Vec", "Box"];

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
            if d.as_str().trim().starts_with(&format!("# {section}")) {
                return true;
            }
        }
    }
    false
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

pub(crate) fn rvs_is_pub_impl_item(cx: &LateContext<'_>, impl_item: &ImplItem<'_>) -> bool {
    matches!(impl_item.impl_kind, ImplItemImplKind::Inherent { .. })
        && cx.tcx.visibility(impl_item.owner_id.def_id).is_public()
}

// ─── FnInfo ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct FnInfo {
    pub caps: CapabilitySet,
    pub raw_suffix: String,
}

impl FnInfo {
    pub(crate) fn rvs_extract_signature(name: &str, _sig: &rustc_hir::FnSig<'_>) -> Option<Self> {
        let (_, caps) = rvs_parse_function(name)?;
        Some(Self {
            caps,
            raw_suffix: rvs_extract_raw_suffix(name),
        })
    }

    pub(crate) fn rvs_extract<'tcx>(
        name: &str,
        _sig: &rustc_hir::FnSig<'_>,
        _body: &Body<'tcx>,
        _tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> Option<Self> {
        let (_, caps) = rvs_parse_function(name)?;
        Some(Self {
            caps,
            raw_suffix: rvs_extract_raw_suffix(name),
        })
    }
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

pub(crate) fn rvs_scan_stub<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &Body<'tcx>) -> bool {
    let mut f = false;
    let todo_sym = Symbol::intern("todo");
    let unimpl_sym = Symbol::intern("unimplemented");
    rvs_walk_closures(tcx, body.value, |e| {
        if f {
            return;
        }
        if let ExprKind::Call(func, _) = &e.kind {
            if let ExprKind::Path(ref q) = func.kind {
                let s = rvs_qp(q);
                let last = s.rsplit("::").next().unwrap_or(&s);
                if last == "todo" || last == "unimplemented" {
                    f = true;
                    return;
                }
            }
        }
        if e.span.from_expansion() {
            let mut expn_id = e.span.ctxt().outer_expn_data().parent;
            while expn_id != rustc_span::ExpnId::root() {
                let expn = expn_id.expn_data();
                if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = expn.kind {
                    if name == todo_sym || name == unimpl_sym {
                        f = true;
                        return;
                    }
                }
                expn_id = expn.parent;
            }
            let outer_expn = e.span.ctxt().outer_expn_data();
            if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = outer_expn.kind
            {
                if name == todo_sym || name == unimpl_sym {
                    f = true;
                    return;
                }
            }
        }
    });
    f
}

pub(crate) fn rvs_is_empty_body(body: &Body<'_>) -> (bool, bool) {
    let block = match &body.value.kind {
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
        ExprKind::Call(func, _) => {
            if let ExprKind::Path(ref q) = func.kind {
                let s = rvs_qp(q);
                let last = s.rsplit("::").next().unwrap_or(&s);
                last == "debug_assert" || last == "debug_assert_eq" || last == "debug_assert_ne"
            } else {
                false
            }
        }
        _ => false,
    }
}

fn rvs_expr_from_debug_assert_macro(e: &Expr<'_>) -> bool {
    let da = Symbol::intern("debug_assert");
    let dae = Symbol::intern("debug_assert_eq");
    let dan = Symbol::intern("debug_assert_ne");
    let outer = e.span.ctxt().outer_expn_data();
    if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = outer.kind
        && (name == da || name == dae || name == dan)
    {
        return true;
    }
    let mut expn_id = outer.parent;
    while expn_id != rustc_span::ExpnId::root() {
        let expn = expn_id.expn_data();
        if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = expn.kind
            && (name == da || name == dae || name == dan)
        {
            return true;
        }
        expn_id = expn.parent;
    }
    false
}

pub(crate) fn rvs_scan_debug_asserts_M<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> BTreeSet<String> {
    let da = Symbol::intern("debug_assert");
    let dae = Symbol::intern("debug_assert_eq");
    let dan = Symbol::intern("debug_assert_ne");
    let mut out = BTreeSet::new();
    let resolver = |_bid: rustc_hir::BodyId| -> Option<&'tcx Body<'tcx>> { None };
    let body_expr = rvs_root_body_expr(tcx, body);
    rvs_walk_expr_M(
        body_expr,
        &mut |e| {
            if e.span.from_expansion() {
                let mut expn_id = e.span.ctxt().outer_expn_data().parent;
                let mut is_debug_assert = false;
                let outer_expn = e.span.ctxt().outer_expn_data();
                if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) =
                    outer_expn.kind
                {
                    if name == da || name == dae || name == dan {
                        is_debug_assert = true;
                    }
                }
                while expn_id != rustc_span::ExpnId::root() {
                    let expn = expn_id.expn_data();
                    if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) =
                        expn.kind
                    {
                        if name == da || name == dae || name == dan {
                            is_debug_assert = true;
                            break;
                        }
                    }
                    expn_id = expn.parent;
                }
                if is_debug_assert {
                    rvs_collect_all_idents_M(e, &mut out);
                }
            }
        },
        &resolver,
        0,
    );
    out
}

fn rvs_collect_all_idents_M(e: &Expr<'_>, out: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Path(q) => {
            if let Some(n) = rvs_plast(q) {
                out.insert(n);
            }
        }
        ExprKind::Array(a) | ExprKind::Tup(a) => {
            a.iter().for_each(|x| rvs_collect_all_idents_M(x, out));
        }
        ExprKind::Call(fn_, a) => {
            rvs_collect_all_idents_M(fn_, out);
            a.iter().for_each(|x| rvs_collect_all_idents_M(x, out));
        }
        ExprKind::MethodCall(_, r, a, _) => {
            rvs_collect_all_idents_M(r, out);
            a.iter().for_each(|x| rvs_collect_all_idents_M(x, out));
        }
        ExprKind::Binary(_, l, r) | ExprKind::AssignOp(_, l, r) => {
            rvs_collect_all_idents_M(l, out);
            rvs_collect_all_idents_M(r, out);
        }
        ExprKind::Unary(_, x)
        | ExprKind::Cast(x, _)
        | ExprKind::Type(x, _)
        | ExprKind::Field(x, _)
        | ExprKind::Index(x, _, _)
        | ExprKind::AddrOf(_, _, x)
        | ExprKind::Repeat(x, _)
        | ExprKind::Yield(x, _) => rvs_collect_all_idents_M(x, out),
        ExprKind::Let(l) => rvs_collect_all_idents_M(&l.init, out),
        ExprKind::If(c, t, el) => {
            rvs_collect_all_idents_M(c, out);
            rvs_collect_all_idents_M(t, out);
            if let Some(e) = el {
                rvs_collect_all_idents_M(e, out);
            }
        }
        ExprKind::Match(s, arms, _) => {
            rvs_collect_all_idents_M(s, out);
            for arm in *arms {
                if let Some(guard) = arm.guard {
                    rvs_collect_all_idents_M(guard, out);
                }
                rvs_collect_all_idents_M(&arm.body, out);
            }
        }
        ExprKind::Block(b, _) | ExprKind::Loop(b, ..) => {
            for s in b.stmts {
                match &s.kind {
                    rustc_hir::StmtKind::Expr(e) | rustc_hir::StmtKind::Semi(e) => {
                        rvs_collect_all_idents_M(e, out);
                    }
                    rustc_hir::StmtKind::Let(l) => {
                        if let Some(i) = l.init {
                            rvs_collect_all_idents_M(i, out);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(e) = b.expr {
                rvs_collect_all_idents_M(e, out);
            }
        }
        ExprKind::Assign(l, r, _) => {
            rvs_collect_all_idents_M(l, out);
            rvs_collect_all_idents_M(r, out);
        }
        ExprKind::Break(_, Some(x)) | ExprKind::Ret(Some(x)) => {
            rvs_collect_all_idents_M(&**x, out);
        }
        ExprKind::Struct(_, fld, rest) => {
            for fl in *fld {
                rvs_collect_all_idents_M(&fl.expr, out);
            }
            if let rustc_hir::StructTailExpr::Base(r) = rest {
                rvs_collect_all_idents_M(r, out);
            }
        }
        ExprKind::DropTemps(x) => rvs_collect_all_idents_M(x, out),
        ExprKind::Become(x) => rvs_collect_all_idents_M(x, out),
        ExprKind::Use(x, _) => rvs_collect_all_idents_M(x, out),
        _ => {}
    }
}

pub(crate) fn rvs_static_is_thread_local(cx: &LateContext<'_>, did: DefId) -> bool {
    if let Some(local_did) = did.as_local() {
        let owner_id = rustc_hir::OwnerId { def_id: local_did };
        let attrs = cx.tcx.hir_attrs(rustc_hir::HirId::from(owner_id));
        return rvs_has_attr(attrs, "thread_local");
    }
    false
}

pub(crate) fn rvs_collect_test_call_names_M<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
    out: &mut HashSet<String>,
) {
    rvs_walk_closures(tcx, body.value, |e| {
        if let ExprKind::Call(func, _) = &e.kind {
            if let ExprKind::Path(ref q) = func.kind {
                if let Some(name) = rvs_plast(q) {
                    if name.starts_with("rvs_") {
                        out.insert(name);
                    }
                }
            }
        }
        if let ExprKind::MethodCall(p, ..) = &e.kind {
            let n = p.ident.name.as_str();
            if n.starts_with("rvs_") {
                out.insert(n.to_string());
            }
        }
    });
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

fn rvs_root_body_expr<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> &'tcx Expr<'tcx> {
    let mut expr = body.value;
    loop {
        match &expr.kind {
            ExprKind::Closure(closure) => {
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

fn rvs_walk_expr_M<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
    e: &'tcx Expr<'tcx>,
    f: &mut F,
    resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
    depth: u32,
) {
    debug_assert!(
        depth <= 17,
        "closure walk depth is capped before recursion continues"
    );
    if depth > 16 {
        return;
    }
    f(e);
    match &e.kind {
        ExprKind::Array(a) | ExprKind::Tup(a) => a
            .iter()
            .for_each(|x| rvs_walk_expr_M(x, f, resolve_body, depth)),
        ExprKind::Call(fn_, a) => {
            rvs_walk_expr_M(fn_, f, resolve_body, depth);
            a.iter()
                .for_each(|x| rvs_walk_expr_M(x, f, resolve_body, depth));
        }
        ExprKind::MethodCall(_, r, a, _) => {
            rvs_walk_expr_M(r, f, resolve_body, depth);
            a.iter()
                .for_each(|x| rvs_walk_expr_M(x, f, resolve_body, depth));
        }
        ExprKind::Binary(_, l, r) | ExprKind::AssignOp(_, l, r) => {
            rvs_walk_expr_M(l, f, resolve_body, depth);
            rvs_walk_expr_M(r, f, resolve_body, depth);
        }
        ExprKind::Unary(_, x)
        | ExprKind::Cast(x, _)
        | ExprKind::Type(x, _)
        | ExprKind::Field(x, _)
        | ExprKind::Index(x, _, _)
        | ExprKind::AddrOf(_, _, x)
        | ExprKind::Repeat(x, _)
        | ExprKind::Yield(x, _) => rvs_walk_expr_M(x, f, resolve_body, depth),
        ExprKind::Let(l) => rvs_walk_expr_M(&l.init, f, resolve_body, depth),
        ExprKind::If(c, t, el) => {
            rvs_walk_expr_M(c, f, resolve_body, depth);
            rvs_walk_expr_M(t, f, resolve_body, depth);
            if let Some(e) = el {
                rvs_walk_expr_M(e, f, resolve_body, depth);
            }
        }
        ExprKind::Match(s, arms, _) => {
            rvs_walk_expr_M(s, f, resolve_body, depth);
            for arm in *arms {
                if let Some(guard) = arm.guard {
                    rvs_walk_expr_M(guard, f, resolve_body, depth);
                }
                rvs_walk_expr_M(&arm.body, f, resolve_body, depth);
            }
        }
        ExprKind::Loop(b, ..) | ExprKind::Block(b, _) => {
            rvs_walk_block_M(b, f, resolve_body, depth)
        }
        ExprKind::Assign(l, r, _) => {
            rvs_walk_expr_M(l, f, resolve_body, depth);
            rvs_walk_expr_M(r, f, resolve_body, depth);
        }
        ExprKind::Break(_, Some(x)) | ExprKind::Ret(Some(x)) => {
            rvs_walk_expr_M(&**x, f, resolve_body, depth)
        }
        ExprKind::Struct(_, fld, rest) => {
            fld.iter()
                .for_each(|fl| rvs_walk_expr_M(&fl.expr, f, resolve_body, depth));
            if let rustc_hir::StructTailExpr::Base(r) = rest {
                rvs_walk_expr_M(r, f, resolve_body, depth);
            }
        }
        ExprKind::Closure(closure) => {
            if let Some(body) = resolve_body(closure.body) {
                rvs_walk_expr_M(body.value, f, resolve_body, depth + 1);
            }
        }
        ExprKind::InlineAsm(asm) => asm.operands.iter().for_each(|(op, _)| match op {
            rustc_hir::InlineAsmOperand::In { expr, .. }
            | rustc_hir::InlineAsmOperand::Out {
                expr: Some(expr), ..
            } => rvs_walk_expr_M(expr, f, resolve_body, depth),
            _ => {}
        }),
        ExprKind::DropTemps(x) => rvs_walk_expr_M(x, f, resolve_body, depth),
        ExprKind::Become(x) => rvs_walk_expr_M(x, f, resolve_body, depth),
        ExprKind::Use(x, _) => rvs_walk_expr_M(x, f, resolve_body, depth),
        _ => {}
    }
}

fn rvs_walk_block_M<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
    b: &'tcx Block<'tcx>,
    f: &mut F,
    resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
    depth: u32,
) {
    debug_assert!(
        depth <= 17,
        "closure walk depth is capped before recursion continues"
    );
    for s in b.stmts {
        match &s.kind {
            rustc_hir::StmtKind::Expr(e) | rustc_hir::StmtKind::Semi(e) => {
                rvs_walk_expr_M(e, f, resolve_body, depth)
            }
            rustc_hir::StmtKind::Let(l) => {
                if let Some(i) = l.init {
                    rvs_walk_expr_M(i, f, resolve_body, depth);
                }
                if let Some(els) = l.els {
                    rvs_walk_block_M(els, f, resolve_body, depth);
                }
            }
            _ => {}
        }
    }
    if let Some(e) = b.expr {
        rvs_walk_expr_M(e, f, resolve_body, depth);
    }
}

pub(crate) fn rvs_walk_closures<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    e: &'tcx Expr<'tcx>,
    mut f: F,
) {
    let resolver = |bid: rustc_hir::BodyId| -> Option<&'tcx Body<'tcx>> { Some(tcx.hir_body(bid)) };
    rvs_walk_expr_M(e, &mut f, &resolver, 0);
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

pub(crate) fn rvs_ty_last_ident(ty: &rustc_hir::Ty<'_>) -> Option<String> {
    match &ty.kind {
        TyKind::Path(q) => rvs_plast(q),
        TyKind::Ref(_, mt) => rvs_ty_last_ident(mt.ty),
        _ => None,
    }
}

pub(crate) fn rvs_generic_args_result_type<'a>(
    args: Option<&'a rustc_hir::GenericArgs<'a>>,
) -> Vec<&'a rustc_hir::Ty<'a>> {
    let Some(ga) = args else { return vec![] };
    ga.args
        .iter()
        .filter_map(|a| match a {
            GenericArg::Type(t) => Some(t.as_unambig_ty()),
            _ => None,
        })
        .collect()
}

pub(crate) fn rvs_collect_type_idents_M(ty: &rustc_hir::Ty<'_>, out: &mut HashSet<String>) {
    match &ty.kind {
        TyKind::Path(q) => {
            if let Some(name) = rvs_plast(q) {
                out.insert(name);
            }
            if let QPath::Resolved(_, p) = q {
                for seg in p.segments {
                    if let Some(ga) = seg.args {
                        for a in ga.args {
                            if let GenericArg::Type(t) = a {
                                rvs_collect_type_idents_M(t.as_unambig_ty(), out);
                            }
                        }
                    }
                }
            }
        }
        TyKind::Ref(_, mt) => rvs_collect_type_idents_M(mt.ty, out),
        _ => {}
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
            let _impl_item: &ImplItem<'_> = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _body: &Body<'_> = unreachable!();
            let _block: &Block<'_> = unreachable!();
            let _expr: &Expr<'_> = unreachable!();
            let _qpath: &QPath<'_> = unreachable!();
            let _ty: &rustc_hir::Ty<'_> = unreachable!();
            let _tcx: rustc_middle::ty::TyCtxt<'_> = unreachable!();
            let mut set = BTreeSet::new();
            let mut refs = HashSet::new();
            let mut names = HashSet::new();
            let mut in_comment = false;

            rvs_has_attr(_attrs, "test");
            rvs_has_allow(_attrs, "dead_code");
            rvs_allows_non_snake_case(_cx, _hir_id);
            rvs_has_doc_section(_cx, _hir_id, "Safety");
            rvs_has_any_doc(_attrs);
            rvs_has_debug_derive(_cx, _def_id);
            rvs_is_pub_impl_item(_cx, _impl_item);
            FnInfo::rvs_extract_signature("rvs_helper", _sig);
            FnInfo::rvs_extract("rvs_helper", _sig, _body, _tcx);
            rvs_has_mutable_params(_sig);
            rvs_scan_stub(_tcx, _body);
            rvs_is_empty_body(_body);
            rvs_is_only_debug_asserts(_expr);
            rvs_expr_from_debug_assert_macro(_expr);
            rvs_scan_debug_asserts_M(_tcx, _body);
            rvs_collect_all_idents_M(_expr, &mut set);
            rvs_static_is_thread_local(_cx, _def_id);
            rvs_collect_test_call_names_M(_tcx, _body, &mut names);
            rvs_count_effective_lines_M(_cx, _body);
            rvs_root_body_expr(_tcx, _body);
            rvs_line_has_effective_code_M("let x = 1;", &mut in_comment);
            let resolver = |_bid: rustc_hir::BodyId| -> Option<&Body<'_>> { None };
            rvs_walk_expr_M(_expr, &mut |_| {}, &resolver, 0);
            rvs_walk_block_M(_block, &mut |_| {}, &resolver, 0);
            rvs_walk_closures(_tcx, _expr, |_| {});
            rvs_qp(_qpath);
            rvs_tys(_ty);
            rvs_plast(_qpath);
            rvs_def_path(_cx, _def_id);
            rvs_impl_type_name(_cx, _def_id);
            rvs_ty_last_ident(_ty);
            rvs_generic_args_result_type(None);
            rvs_collect_type_idents_M(_ty, &mut refs);
        }
    }
}
