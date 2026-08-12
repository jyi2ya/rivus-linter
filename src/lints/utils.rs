use std::collections::HashSet;

use rustc_hir::{
    self, AmbigArg, Body, BodyId, Expr, ExprKind, HirId, InlineAsm, InlineAsmOperand, Mutability,
    Pat, PathSegment, QPath, Ty, TyKind,
    attrs::AttributeKind,
    def::DefKind,
    def_id::{CrateNum, DefId, LocalDefId},
    intravisit::{self, Visitor},
};
use rustc_lexer::{FrontmatterAllowed, TokenKind};
use rustc_lint::LateContext;
use rustc_middle::{
    hir::nested_filter,
    ty::{self, Ty as MiddleTy, TyCtxt, TypeSuperVisitable, TypeVisitable, TypeVisitor},
};
use rustc_span::{Span, Symbol};

use super::body::macro_expansion::rvs_span_has_bang_macro;
use crate::symbols::{DefPath, rvs_attach_generated_definition_marker_M};

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

#[derive(Debug)]
struct ImplTypeIdentity {
    readable_path: String,
    marker: String,
    is_nominal_path: bool,
}

#[allow(
    clippy::allow_attributes,
    reason = "rustc TyCtxt does not implement Debug"
)]
struct ImplNominalIdentityVisitor<'tcx> {
    tcx: TyCtxt<'tcx>,
    impl_crate: CrateNum,
    identities: Vec<String>,
}

impl<'tcx> ImplNominalIdentityVisitor<'tcx> {
    fn rvs_new(tcx: TyCtxt<'tcx>, impl_crate: CrateNum) -> Self {
        Self {
            tcx,
            impl_crate,
            identities: Vec::new(),
        }
    }

    fn rvs_record_M(&mut self, kind: &str, def_id: DefId) {
        debug_assert!(!kind.is_empty(), "nominal identity kind is nonempty");
        let crate_identity = if def_id.krate == self.impl_crate {
            "local".to_string()
        } else {
            let crate_id = self.tcx.stable_crate_id(def_id.krate).as_u64();
            debug_assert!(crate_id > 0, "external stable crate id is nonzero");
            format!("{crate_id:016x}")
        };
        self.identities.push(format!("{kind}:{crate_identity}"));
    }

    fn rvs_finish(self) -> String {
        self.identities.join(",")
    }
}

impl<'tcx> TypeVisitor<TyCtxt<'tcx>> for ImplNominalIdentityVisitor<'tcx> {
    fn visit_ty(&mut self, ty: MiddleTy<'tcx>) {
        match *ty.kind() {
            ty::TyKind::Adt(definition, _) => self.rvs_record_M("adt", definition.did()),
            ty::TyKind::Foreign(def_id) => self.rvs_record_M("foreign", def_id),
            ty::TyKind::FnDef(def_id, _) => self.rvs_record_M("fn", def_id),
            ty::TyKind::Closure(def_id, _) => self.rvs_record_M("closure", def_id),
            ty::TyKind::CoroutineClosure(def_id, _) => {
                self.rvs_record_M("coroutine-closure", def_id);
            }
            ty::TyKind::Coroutine(def_id, _) => self.rvs_record_M("coroutine", def_id),
            ty::TyKind::CoroutineWitness(def_id, _) => {
                self.rvs_record_M("coroutine-witness", def_id);
            }
            ty::TyKind::Alias(alias) => {
                self.rvs_record_M("alias", alias.kind.def_id());
            }
            ty::TyKind::Dynamic(predicates, _) => {
                for predicate in predicates {
                    match predicate.skip_binder() {
                        ty::ExistentialPredicate::Trait(trait_ref) => {
                            self.rvs_record_M("trait", trait_ref.def_id);
                        }
                        ty::ExistentialPredicate::Projection(projection) => {
                            self.rvs_record_M("associated-type", projection.def_id);
                            self.rvs_record_M("trait", self.tcx.parent(projection.def_id));
                        }
                        ty::ExistentialPredicate::AutoTrait(def_id) => {
                            self.rvs_record_M("trait", def_id);
                        }
                    }
                }
            }
            _ => {}
        }
        ty.super_visit_with(self);
    }

    fn visit_const(&mut self, constant: ty::Const<'tcx>) {
        if let ty::ConstKind::Unevaluated(unevaluated) = constant.kind() {
            self.rvs_record_M("const", unevaluated.def);
        }
        constant.super_visit_with(self);
    }

    fn visit_predicate(&mut self, predicate: ty::Predicate<'tcx>) {
        match predicate.kind().skip_binder() {
            ty::PredicateKind::Clause(ty::ClauseKind::Trait(trait_predicate)) => {
                self.rvs_record_M("trait", trait_predicate.trait_ref.def_id);
            }
            ty::PredicateKind::Clause(ty::ClauseKind::HostEffect(host_effect)) => {
                self.rvs_record_M("trait", host_effect.trait_ref.def_id);
            }
            ty::PredicateKind::Clause(ty::ClauseKind::Projection(projection)) => {
                self.rvs_record_M("associated-type", projection.projection_term.def_id);
                self.rvs_record_M("trait", projection.trait_def_id(self.tcx));
            }
            ty::PredicateKind::DynCompatible(def_id) => {
                self.rvs_record_M("trait", def_id);
            }
            ty::PredicateKind::NormalizesTo(normalizes) => {
                self.rvs_record_M("alias", normalizes.alias.def_id);
            }
            _ => {}
        }
        predicate.super_visit_with(self);
    }
}

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
                let (only_debug_asserts, child_has_debug_assert) = rvs_debug_assert_only(tcx, e);
                if !only_debug_asserts {
                    return (false, false);
                }
                found_debug_assert |= child_has_debug_assert;
            }
            rustc_hir::StmtKind::Let(_) | rustc_hir::StmtKind::Item(_) => return (false, false),
        }
    }
    if let Some(e) = block.expr {
        let (only_debug_asserts, child_has_debug_assert) = rvs_debug_assert_only(tcx, e);
        if !only_debug_asserts {
            return (false, false);
        }
        found_debug_assert |= child_has_debug_assert;
    }
    (true, found_debug_assert)
}

fn rvs_debug_assert_only(tcx: rustc_middle::ty::TyCtxt<'_>, e: &Expr<'_>) -> (bool, bool) {
    if rvs_expr_from_debug_assert_macro(tcx, e) {
        return (true, true);
    }
    match &e.kind {
        ExprKind::Block(b, _) => {
            let mut found_debug_assert = false;
            for s in b.stmts {
                match &s.kind {
                    rustc_hir::StmtKind::Expr(e2) | rustc_hir::StmtKind::Semi(e2) => {
                        let (only_debug_asserts, child_has_debug_assert) =
                            rvs_debug_assert_only(tcx, e2);
                        if !only_debug_asserts {
                            return (false, false);
                        }
                        found_debug_assert |= child_has_debug_assert;
                    }
                    _ => return (false, false),
                }
            }
            if let Some(e) = b.expr {
                let (only_debug_asserts, child_has_debug_assert) = rvs_debug_assert_only(tcx, e);
                if !only_debug_asserts {
                    return (false, false);
                }
                found_debug_assert |= child_has_debug_assert;
            }
            (true, found_debug_assert)
        }
        _ => (false, false),
    }
}

fn rvs_expr_from_debug_assert_macro(tcx: rustc_middle::ty::TyCtxt<'_>, e: &Expr<'_>) -> bool {
    let names = [
        Symbol::intern("debug_assert"),
        Symbol::intern("debug_assert_eq"),
        Symbol::intern("debug_assert_ne"),
    ];
    rvs_span_has_bang_macro(tcx, e.span, names.as_slice())
}

macro_rules! rvs_impl_suppress_noop_visitor_methods {
    ($lt:lifetime) => {
        fn visit_ty(&mut self, _ty: &$lt Ty<$lt, AmbigArg>) {}
        fn visit_qpath(&mut self, _qpath: &$lt QPath<$lt>, _id: HirId, _span: Span) {}
        fn visit_path_segment(&mut self, _segment: &$lt PathSegment<$lt>) {}
    };
}

fn rvs_walk_inline_asm_operand_exprs_M<'v, V: Visitor<'v, Result = ()>>(
    visitor: &mut V,
    asm: &'v InlineAsm<'v>,
) {
    for (operand, _) in asm.operands {
        match operand {
            InlineAsmOperand::In { expr, .. }
            | InlineAsmOperand::InOut { expr, .. }
            | InlineAsmOperand::Out {
                expr: Some(expr), ..
            }
            | InlineAsmOperand::SymFn { expr } => visitor.visit_expr(expr),
            InlineAsmOperand::SplitInOut {
                in_expr, out_expr, ..
            } => {
                visitor.visit_expr(in_expr);
                if let Some(expr) = out_expr {
                    visitor.visit_expr(expr);
                }
            }
            InlineAsmOperand::Out { expr: None, .. }
            | InlineAsmOperand::Const { .. }
            | InlineAsmOperand::SymStatic { .. }
            | InlineAsmOperand::Label { .. } => {}
        }
    }
}

#[allow(
    clippy::allow_attributes,
    reason = "rustc LateContext does not implement Debug"
)]
struct LocalBindingVisitor<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    out: &'a mut HashSet<HirId>,
}

impl<'hir, 'tcx> Visitor<'hir> for LocalBindingVisitor<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'hir Expr<'hir>) {
        if let ExprKind::Path(qpath) = &expr.kind
            && let rustc_hir::def::Res::Local(binding_hir_id) =
                self.cx.qpath_res(qpath, expr.hir_id)
        {
            self.out.insert(binding_hir_id);
        }
        match expr.kind {
            ExprKind::Closure(_) => {}
            ExprKind::Assign(lhs, rhs, _) | ExprKind::AssignOp(_, lhs, rhs) => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            _ => intravisit::walk_expr(self, expr),
        }
    }

    fn visit_nested_body(&mut self, _body_id: BodyId) {}
    fn visit_pat(&mut self, _pat: &'hir Pat<'hir>) {}

    rvs_impl_suppress_noop_visitor_methods!('hir);

    fn visit_inline_asm(&mut self, asm: &'hir InlineAsm<'hir>, _id: HirId) {
        rvs_walk_inline_asm_operand_exprs_M(self, asm);
    }
}

pub(crate) fn rvs_collect_local_bindings_M(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
    out: &mut HashSet<HirId>,
) {
    LocalBindingVisitor { cx, out }.visit_expr(expr);
}

pub(crate) fn rvs_static_is_thread_local(cx: &LateContext<'_>, did: DefId) -> bool {
    cx.tcx.is_thread_local_static(did)
}

pub(crate) fn rvs_is_sysroot_crate_id(
    cx: &LateContext<'_>,
    crate_id: u64,
    crate_name: &str,
) -> bool {
    debug_assert!(crate_id > 0, "stable crate identity must be nonzero");
    let marker = match crate_name {
        "core" => cx.tcx.lang_items().sized_trait(),
        "alloc" => cx.tcx.lang_items().owned_box(),
        "std" => cx.tcx.get_diagnostic_item(Symbol::intern("File")),
        _ => return false,
    };
    marker.is_some_and(|def_id| cx.tcx.stable_crate_id(def_id.krate).as_u64() == crate_id)
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
    rvs_count_effective_lines(&snippet)
}

fn rvs_count_effective_lines(snippet: &str) -> usize {
    let mut uncommented = snippet.as_bytes().to_vec();
    let mut offset = 0usize;
    for token in rustc_lexer::tokenize(snippet, FrontmatterAllowed::No) {
        let Ok(token_len) = usize::try_from(token.len) else {
            return 0;
        };
        let Some(end) = offset.checked_add(token_len) else {
            return 0;
        };
        if matches!(
            token.kind,
            TokenKind::LineComment { .. } | TokenKind::BlockComment { .. }
        ) {
            let Some(comment) = uncommented.get_mut(offset..end) else {
                return 0;
            };
            for byte in comment {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        }
        offset = end;
    }
    debug_assert_eq!(offset, uncommented.len(), "lexer covers the full snippet");
    uncommented
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            let Some(start) = line.iter().position(|byte| !byte.is_ascii_whitespace()) else {
                return false;
            };
            let end = line
                .iter()
                .rposition(|byte| !byte.is_ascii_whitespace())
                .expect("never: non-whitespace line has an end");
            let trimmed = line
                .get(start..=end)
                .expect("never: detected line bounds are valid");
            trimmed != b"{" && trimmed != b"}"
        })
        .count()
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

#[allow(
    clippy::allow_attributes,
    reason = "rustc TyCtxt does not implement Debug"
)]
struct BodyExprVisitor<'a, 'tcx, F> {
    tcx: TyCtxt<'tcx>,
    callback: &'a mut F,
    body_owner: LocalDefId,
}

impl<'tcx, F> Visitor<'tcx> for BodyExprVisitor<'_, 'tcx, F>
where
    F: FnMut(&'tcx Expr<'tcx>, LocalDefId),
{
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_nested_body(&mut self, body_id: BodyId) {
        let body = self.tcx.hir_body(body_id);
        let previous = std::mem::replace(&mut self.body_owner, body_id.hir_id.owner.def_id);
        self.visit_expr(body.value);
        self.body_owner = previous;
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        (self.callback)(expr, self.body_owner);
        match expr.kind {
            ExprKind::Closure(closure) => {
                let previous = std::mem::replace(&mut self.body_owner, closure.def_id);
                self.visit_expr(self.tcx.hir_body(closure.body).value);
                self.body_owner = previous;
            }
            ExprKind::Assign(lhs, rhs, _) | ExprKind::AssignOp(_, lhs, rhs) => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            ExprKind::ConstBlock(_) => {}
            ExprKind::Repeat(element, _) => {
                self.visit_expr(element);
            }
            _ => intravisit::walk_expr(self, expr),
        }
    }

    rvs_impl_suppress_noop_visitor_methods!('tcx);

    fn visit_pat(&mut self, pat: &'tcx Pat<'tcx>) {
        intravisit::walk_pat(self, pat);
    }

    fn visit_inline_asm(&mut self, asm: &'tcx InlineAsm<'tcx>, _id: HirId) {
        for (operand, _) in asm.operands {
            match operand {
                InlineAsmOperand::In { expr, .. }
                | InlineAsmOperand::InOut { expr, .. }
                | InlineAsmOperand::Out {
                    expr: Some(expr), ..
                }
                | InlineAsmOperand::SymFn { expr } => self.visit_expr(expr),
                InlineAsmOperand::SplitInOut {
                    in_expr, out_expr, ..
                } => {
                    self.visit_expr(in_expr);
                    if let Some(expr) = out_expr {
                        self.visit_expr(expr);
                    }
                }
                InlineAsmOperand::Label { block } => self.visit_block(block),
                InlineAsmOperand::Const { .. }
                | InlineAsmOperand::Out { expr: None, .. }
                | InlineAsmOperand::SymStatic { .. } => {}
            }
        }
    }
}

pub(crate) fn rvs_visit_body_exprs_M<'tcx, F>(
    tcx: TyCtxt<'tcx>,
    expr: &'tcx Expr<'tcx>,
    mut callback: F,
) where
    F: FnMut(&'tcx Expr<'tcx>, LocalDefId),
{
    BodyExprVisitor {
        tcx,
        callback: &mut callback,
        body_owner: expr.hir_id.owner.def_id,
    }
    .visit_expr(expr);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallSyntax {
    Function,
    Method,
}

/// How a call observation was discovered in HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationKind {
    /// A direct `ExprKind::Call` or `ExprKind::MethodCall`.
    Direct,
    /// A function-item path referenced but not directly called (function pointer,
    /// callback argument, etc.).
    FunctionReference,
    /// A call through a local callable whose concrete target cannot be resolved
    /// at the HIR level — e.g. `f()` where `f` is a `fn()` parameter, a closure
    /// variable, or a generic `F: Fn()` parameter.
    UnsupportedIndirect,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub(crate) struct CallObservation {
    pub kind: ObservationKind,
    pub syntax: CallSyntax,
    pub target: CallTarget,
    pub hir_id: HirId,
    pub span: Span,
    pub body_owner: LocalDefId,
}

pub(crate) fn rvs_resolve_call(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<CallObservation> {
    match &expr.kind {
        ExprKind::Call(func, _) => {
            let mut callee = *func;
            loop {
                match callee.kind {
                    ExprKind::Cast(inner, _)
                    | ExprKind::Type(inner, _)
                    | ExprKind::DropTemps(inner)
                    | ExprKind::Use(inner, _)
                    | ExprKind::UnsafeBinderCast(_, inner, _) => callee = inner,
                    ExprKind::Block(block, _) => {
                        let Some(inner) = block.expr else {
                            break;
                        };
                        callee = inner;
                    }
                    _ => break,
                }
            }
            let ExprKind::Path(qpath) = &callee.kind else {
                return Some(CallObservation {
                    kind: ObservationKind::UnsupportedIndirect,
                    syntax: CallSyntax::Function,
                    target: CallTarget::UnresolvedPath {
                        path: String::new(),
                    },
                    hir_id: expr.hir_id,
                    span: expr.span,
                    body_owner: expr.hir_id.owner.def_id,
                });
            };
            let target = match cx.qpath_res(qpath, callee.hir_id) {
                rustc_hir::def::Res::Def(def_kind, def_id)
                    if rvs_def_id_is_fn_trait_operation(cx, def_id)
                        || matches!(
                            def_kind,
                            DefKind::Const { .. }
                                | DefKind::AssocConst { .. }
                                | DefKind::Static { .. }
                        ) =>
                {
                    return Some(CallObservation {
                        kind: ObservationKind::UnsupportedIndirect,
                        syntax: CallSyntax::Function,
                        target: CallTarget::UnresolvedPath {
                            path: rvs_qp(qpath),
                        },
                        hir_id: expr.hir_id,
                        span: expr.span,
                        body_owner: expr.hir_id.owner.def_id,
                    });
                }
                rustc_hir::def::Res::Def(def_kind, def_id) => CallTarget::Resolved {
                    def_path: DefPath::rvs_new(rvs_def_path(cx, def_id)),
                    def_kind,
                    crate_id: cx.tcx.stable_crate_id(def_id.krate).as_u64(),
                },
                rustc_hir::def::Res::Local(_) => {
                    return Some(CallObservation {
                        kind: ObservationKind::UnsupportedIndirect,
                        syntax: CallSyntax::Function,
                        target: CallTarget::UnresolvedPath {
                            path: rvs_qp(qpath),
                        },
                        hir_id: expr.hir_id,
                        span: expr.span,
                        body_owner: expr.hir_id.owner.def_id,
                    });
                }
                _ => CallTarget::UnresolvedPath {
                    path: rvs_qp(qpath),
                },
            };
            Some(CallObservation {
                kind: ObservationKind::Direct,
                syntax: CallSyntax::Function,
                target,
                hir_id: expr.hir_id,
                span: expr.span,
                body_owner: expr.hir_id.owner.def_id,
            })
        }
        ExprKind::MethodCall(path, ..) => {
            let owner = expr.hir_id.owner.def_id;
            let typeck = cx.tcx.typeck(owner);
            let resolved_def_id = typeck.type_dependent_def_id(expr.hir_id);
            if resolved_def_id.is_some_and(|def_id| rvs_def_id_is_fn_trait_operation(cx, def_id)) {
                return Some(CallObservation {
                    kind: ObservationKind::UnsupportedIndirect,
                    syntax: CallSyntax::Method,
                    target: CallTarget::UnresolvedMethod {
                        name: path.ident.name.to_string(),
                    },
                    hir_id: expr.hir_id,
                    span: expr.span,
                    body_owner: expr.hir_id.owner.def_id,
                });
            }
            let resolved = resolved_def_id.map(|def_id| {
                (
                    DefPath::rvs_new(rvs_def_path(cx, def_id)),
                    cx.tcx.def_kind(def_id),
                    cx.tcx.stable_crate_id(def_id.krate).as_u64(),
                )
            });
            Some(CallObservation {
                kind: ObservationKind::Direct,
                syntax: CallSyntax::Method,
                target: rvs_method_call_target(resolved, path.ident.name.as_str()),
                hir_id: expr.hir_id,
                span: expr.span,
                body_owner: expr.hir_id.owner.def_id,
            })
        }
        _ => None,
    }
}

pub(crate) fn rvs_def_id_is_fn_trait_operation(cx: &LateContext<'_>, def_id: DefId) -> bool {
    cx.tcx.trait_of_assoc(def_id).is_some_and(|trait_def_id| {
        cx.tcx.is_lang_item(trait_def_id, rustc_hir::LangItem::Fn)
            || cx
                .tcx
                .is_lang_item(trait_def_id, rustc_hir::LangItem::FnMut)
            || cx
                .tcx
                .is_lang_item(trait_def_id, rustc_hir::LangItem::FnOnce)
    })
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

pub(crate) fn rvs_def_path(cx: &LateContext<'_>, did: DefId) -> String {
    let tcx = cx.tcx;
    let dp = tcx.def_path(did);
    let impl_def_id = rvs_enclosing_impl_def_id(cx, did);
    let impl_ty = impl_def_id.and_then(|impl_def_id| rvs_impl_type_identity(cx, impl_def_id));
    let is_sourceless = tcx.def_span(did).is_dummy();
    let definition_marker = cx
        .tcx
        .opt_associated_item(did)
        .is_none()
        .then(|| rvs_definition_identity(cx, did))
        .flatten()
        .map(|identity| rvs_encode_identity_marker(&identity));

    let mut parts = vec![tcx.crate_name(dp.krate).to_string()];
    let mut has_impl = false;
    for d in &dp.data {
        match d.data {
            rustc_hir::definitions::DefPathData::TypeNs(s)
            | rustc_hir::definitions::DefPathData::MacroNs(s) => {
                parts.push(s.to_string());
            }
            rustc_hir::definitions::DefPathData::ValueNs(s) => {
                let mut value = s.to_string();
                if is_sourceless && d.disambiguator > 0 {
                    value.push('#');
                    value.push_str(&d.disambiguator.to_string());
                }
                parts.push(value);
            }
            rustc_hir::definitions::DefPathData::Impl => {
                if let Some(identity) = &impl_ty {
                    if identity.is_nominal_path {
                        parts = identity
                            .readable_path
                            .split("::")
                            .map(str::to_string)
                            .collect();
                        if let Some(type_name) = parts.last_mut() {
                            type_name.push_str(&format!("{{impl#{}}}", identity.marker));
                        }
                    } else {
                        parts.push(format!(
                            "{}{{impl#{}}}",
                            identity.readable_path, identity.marker
                        ));
                    }
                }
                has_impl = true;
            }
            rustc_hir::definitions::DefPathData::Closure => {
                if definition_marker.is_some() {
                    parts.push("closure".to_string());
                } else {
                    parts.push(format!("closure#{}", d.disambiguator));
                }
            }
            _ => {}
        }
    }
    if let Some(marker) = definition_marker {
        rvs_attach_generated_definition_marker_M(&mut parts, &marker);
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

fn rvs_definition_identity(cx: &LateContext<'_>, did: DefId) -> Option<String> {
    let definition_span = cx.tcx.def_span(did);
    if definition_span.from_expansion() {
        return rvs_generated_definition_identity(cx, did);
    }
    if !rvs_is_body_nested_definition(cx, did) {
        return None;
    }
    rvs_span_source_identity(cx, definition_span)
        .map(|definition| format!("definition={definition}"))
}

fn rvs_is_body_nested_definition(cx: &LateContext<'_>, did: DefId) -> bool {
    if did.is_crate_root() {
        return false;
    }
    matches!(
        cx.tcx.def_kind(cx.tcx.parent(did)),
        rustc_hir::def::DefKind::Fn
            | rustc_hir::def::DefKind::AssocFn
            | rustc_hir::def::DefKind::Const { .. }
            | rustc_hir::def::DefKind::AssocConst { .. }
            | rustc_hir::def::DefKind::Static { .. }
            | rustc_hir::def::DefKind::AnonConst
            | rustc_hir::def::DefKind::InlineConst
            | rustc_hir::def::DefKind::Closure
            | rustc_hir::def::DefKind::SyntheticCoroutineBody
    )
}

fn rvs_generated_definition_identity(cx: &LateContext<'_>, did: DefId) -> Option<String> {
    let mut identity = rvs_generated_definition_base_identity(cx, did)?;
    if let Some(ordinal) = rvs_generated_definition_repetition_ordinal(cx, did, &identity) {
        identity.push_str("|same-source-ordinal=");
        identity.push_str(&ordinal.to_string());
    }
    Some(identity)
}

fn rvs_generated_definition_base_identity(cx: &LateContext<'_>, did: DefId) -> Option<String> {
    let definition_span = cx.tcx.def_span(did);
    if !definition_span.from_expansion() {
        return None;
    }

    let mut identity = Vec::new();
    if let Some(definition) = rvs_span_source_identity(cx, definition_span) {
        identity.push(format!("definition={definition}"));
    }

    let mut expansion = definition_span.ctxt().outer_expn();
    while expansion != rustc_span::ExpnId::root() {
        let data = expansion.expn_data();
        let mut component = format!("kind={}", data.kind.descr());
        if let Some(macro_def_id) = data.macro_def_id {
            component.push_str(";macro=");
            component.push_str(cx.tcx.crate_name(macro_def_id.krate).as_str());
            component.push_str(&cx.tcx.def_path(macro_def_id).to_string_no_crate_verbose());
            if let Some(definition) = rvs_span_source_identity(cx, cx.tcx.def_span(macro_def_id)) {
                component.push_str(";macro_definition=");
                component.push_str(&definition);
            }
        }
        if let Some(call_site) = rvs_span_source_identity(cx, data.call_site) {
            component.push_str(";call_site=");
            component.push_str(&call_site);
        }
        identity.push(component);

        let parent_expansion = data.call_site.ctxt().outer_expn();
        if parent_expansion == expansion {
            break;
        }
        expansion = parent_expansion;
    }

    (!identity.is_empty()).then(|| identity.join("|"))
}

fn rvs_generated_definition_repetition_ordinal(
    cx: &LateContext<'_>,
    did: DefId,
    base_identity: &str,
) -> Option<usize> {
    debug_assert!(
        !base_identity.is_empty(),
        "generated base identity is nonempty"
    );
    let local_did = did.as_local()?;
    let definition_kind = cx.tcx.def_kind(did);
    let mut matching_definitions = 0usize;
    let mut ordinal = None;
    for owner in cx.tcx.hir_crate_items(()).owners() {
        let candidate = owner.def_id.to_def_id();
        if cx.tcx.def_kind(candidate) != definition_kind
            || rvs_generated_definition_base_identity(cx, candidate).as_deref()
                != Some(base_identity)
        {
            continue;
        }
        if owner.def_id == local_did {
            ordinal = Some(matching_definitions);
        }
        matching_definitions += 1;
    }
    if matching_definitions > 1 {
        ordinal
    } else {
        None
    }
}

fn rvs_span_source_identity(cx: &LateContext<'_>, span: Span) -> Option<String> {
    if span.is_dummy() {
        return None;
    }
    let source_map = cx.tcx.sess.source_map();
    let span = span.data();
    let start = source_map.lookup_byte_offset(span.lo);
    let end = source_map.lookup_byte_offset(span.hi);
    if start.sf.name != end.sf.name {
        return None;
    }
    Some(format!(
        "{:?}:{}:{}:{}",
        start
            .sf
            .name
            .prefer_remapped_unconditionally()
            .to_string_lossy(),
        start.sf.src_hash,
        start.pos.0,
        end.pos.0,
    ))
}

fn rvs_enclosing_impl_def_id(cx: &LateContext<'_>, did: DefId) -> Option<DefId> {
    let mut current = did;
    loop {
        if matches!(
            cx.tcx.def_kind(current),
            rustc_hir::def::DefKind::Impl { .. }
        ) {
            return Some(current);
        }
        if current.is_crate_root() {
            return None;
        }
        current = cx.tcx.parent(current);
    }
}

fn rvs_type_nominal_crate_identities<'tcx>(
    cx: &LateContext<'tcx>,
    impl_crate: CrateNum,
    ty: MiddleTy<'tcx>,
) -> String {
    let mut visitor = ImplNominalIdentityVisitor::rvs_new(cx.tcx, impl_crate);
    ty.visit_with(&mut visitor);
    visitor.rvs_finish()
}

fn rvs_trait_nominal_crate_identities<'tcx>(
    cx: &LateContext<'tcx>,
    impl_crate: CrateNum,
    trait_ref: ty::TraitRef<'tcx>,
) -> String {
    let mut visitor = ImplNominalIdentityVisitor::rvs_new(cx.tcx, impl_crate);
    visitor.rvs_record_M("trait", trait_ref.def_id);
    trait_ref.args.visit_with(&mut visitor);
    visitor.rvs_finish()
}

fn rvs_predicate_nominal_crate_identities<'tcx>(
    cx: &LateContext<'tcx>,
    impl_crate: CrateNum,
    predicates: &[ty::Clause<'tcx>],
) -> String {
    let mut visitor = ImplNominalIdentityVisitor::rvs_new(cx.tcx, impl_crate);
    predicates.visit_with(&mut visitor);
    visitor.rvs_finish()
}

fn rvs_impl_type_identity(cx: &LateContext<'_>, impl_def_id: DefId) -> Option<ImplTypeIdentity> {
    let self_ty = cx.tcx.type_of(impl_def_id).skip_binder();
    let self_type_text = rustc_middle::ty::print::with_resolve_crate_name!(
        rustc_middle::ty::print::with_no_visible_paths!(
            rustc_middle::ty::print::with_no_trimmed_paths!(self_ty.to_string())
        )
    );
    let self_nominal_crate_identities =
        rvs_type_nominal_crate_identities(cx, impl_def_id.krate, self_ty);
    let (readable_path, is_nominal_path, generated_self_type_identity) = match self_ty.kind() {
        rustc_middle::ty::TyKind::Adt(adt_def, _) => (
            rvs_def_path(cx, adt_def.did()),
            true,
            rvs_generated_definition_identity(cx, adt_def.did()),
        ),
        _ => (
            self_type_text.rsplit("::").next().map(str::to_string)?,
            false,
            None,
        ),
    };
    let impl_predicates = cx
        .tcx
        .predicates_of(impl_def_id)
        .instantiate_identity(cx.tcx);
    let impl_predicates_text = rustc_middle::ty::print::with_resolve_crate_name!(
        rustc_middle::ty::print::with_no_visible_paths!(
            rustc_middle::ty::print::with_no_trimmed_paths!(
                impl_predicates
                    .predicates
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        )
    );
    let predicate_nominal_crate_identities =
        rvs_predicate_nominal_crate_identities(cx, impl_def_id.krate, &impl_predicates.predicates);
    let trait_identity =
        if let rustc_hir::def::DefKind::Impl { of_trait: true } = cx.tcx.def_kind(impl_def_id) {
            let trait_ref = cx.tcx.impl_trait_ref(impl_def_id);
            let trait_ref = trait_ref.skip_binder();
            let trait_text = rustc_middle::ty::print::with_resolve_crate_name!(
                rustc_middle::ty::print::with_no_visible_paths!(
                    rustc_middle::ty::print::with_no_trimmed_paths!(trait_ref.to_string())
                )
            );
            Some((
                trait_text,
                rvs_trait_nominal_crate_identities(cx, impl_def_id.krate, trait_ref),
            ))
        } else {
            None
        };
    let generated_impl_identity = rvs_generated_definition_identity(cx, impl_def_id);
    let generated_definition_identity = generated_impl_identity.or(generated_self_type_identity);
    let marker = rvs_impl_identity_marker(
        &self_type_text,
        &self_nominal_crate_identities,
        trait_identity
            .as_ref()
            .map(|(trait_text, crate_identities)| (trait_text.as_str(), crate_identities.as_str())),
        &impl_predicates_text,
        &predicate_nominal_crate_identities,
        generated_definition_identity.as_deref(),
    );
    Some(ImplTypeIdentity {
        readable_path,
        marker,
        is_nominal_path,
    })
}

fn rvs_impl_identity_marker(
    self_type_text: &str,
    self_nominal_crate_identities: &str,
    trait_identity: Option<(&str, &str)>,
    impl_predicates_text: &str,
    predicate_nominal_crate_identities: &str,
    generated_definition_identity: Option<&str>,
) -> String {
    debug_assert!(!self_type_text.is_empty(), "self type identity is nonempty");
    debug_assert!(
        trait_identity.is_none_or(|(trait_text, _)| !trait_text.is_empty()),
        "trait identity text is nonempty"
    );
    let mut identity_text = self_type_text.to_string();
    if !self_nominal_crate_identities.is_empty() {
        identity_text.push_str("@self-nominal-crates=");
        identity_text.push_str(self_nominal_crate_identities);
    }
    if let Some((trait_text, nominal_crate_identities)) = trait_identity {
        identity_text.push('@');
        identity_text.push_str(trait_text);
        if !nominal_crate_identities.is_empty() {
            identity_text.push_str("@trait-nominal-crates=");
            identity_text.push_str(nominal_crate_identities);
        }
    }
    if !impl_predicates_text.is_empty() {
        identity_text.push_str("@predicates=");
        identity_text.push_str(impl_predicates_text);
    }
    if !predicate_nominal_crate_identities.is_empty() {
        identity_text.push_str("@predicate-nominal-crates=");
        identity_text.push_str(predicate_nominal_crate_identities);
    }
    if let Some(generated_definition_identity) = generated_definition_identity {
        identity_text.push_str("@generated-definition=");
        identity_text.push_str(generated_definition_identity);
    }
    rvs_encode_identity_marker(&identity_text)
}

fn rvs_encode_identity_marker(identity_text: &str) -> String {
    let mut marker = String::new();
    for byte in identity_text.bytes() {
        marker.push_str(&format!("{byte:02x}"));
    }
    marker
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
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};

    #[test]
    fn test_20260715_effective_line_count_uses_rust_lexer() {
        let cases = [
            (
                "nested_comments",
                "{\n    let value = 1;\n    /* outer\n       /* nested */\n       still comment\n    */\n    value\n}\n",
                2,
            ),
            (
                "raw_string",
                "{\n    let raw = r#\"\n// string data\n/* string data */\n\"#;\n    raw\n}\n",
                5,
            ),
            (
                "trailing_comment",
                "{\n    let value = 1; // explanation\n}\n",
                1,
            ),
        ];
        let output = cases
            .iter()
            .map(|(name, snippet, _)| format!("{name}={}", rvs_count_effective_lines(snippet)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS(
            "test_20260715_effective_line_count_uses_rust_lexer",
            &output,
        );

        for (name, snippet, expected) in cases {
            assert_eq!(rvs_count_effective_lines(snippet), expected, "{name}");
        }
    }

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
    fn test_20260730_impl_marker_distinguishes_defining_crates() {
        let first = rvs_impl_identity_marker(
            "app::Worker",
            "adt:000000000000000b",
            Some(("dependency::api::Runner", "trait:0000000000000015")),
            "",
            "",
            None,
        );
        let same = rvs_impl_identity_marker(
            "app::Worker",
            "adt:000000000000000b",
            Some(("dependency::api::Runner", "trait:0000000000000015")),
            "",
            "",
            None,
        );
        let other_trait_version = rvs_impl_identity_marker(
            "app::Worker",
            "adt:000000000000000b",
            Some(("dependency::api::Runner", "trait:0000000000000016")),
            "",
            "",
            None,
        );
        let other_self_version = rvs_impl_identity_marker(
            "app::Worker",
            "adt:000000000000000c",
            Some(("dependency::api::Runner", "trait:0000000000000015")),
            "",
            "",
            None,
        );
        let local_production = rvs_impl_identity_marker(
            "app::Worker",
            "adt:local",
            Some(("app::Runner", "trait:local")),
            "",
            "",
            None,
        );
        let local_test = rvs_impl_identity_marker(
            "app::Worker",
            "adt:local",
            Some(("app::Runner", "trait:local")),
            "",
            "",
            None,
        );
        let output = format!(
            "same_input={}\ntrait_versions_distinct={}\nself_versions_distinct={}\nlocal_targets_stable={}\n",
            first == same,
            first != other_trait_version,
            first != other_self_version,
            local_production == local_test,
        );
        rvs_snapshot_BIS(
            "test_20260730_impl_marker_distinguishes_defining_crates",
            &output,
        );

        assert_eq!(first, same);
        assert_ne!(first, other_trait_version);
        assert_ne!(first, other_self_version);
        assert_eq!(local_production, local_test);
    }

    #[test]
    fn test_20260731_impl_marker_distinguishes_specialization_predicates() {
        let clone_impl = rvs_impl_identity_marker(
            "T",
            "",
            Some(("app::Specialized", "trait:local")),
            "T: app::Clone",
            "trait:local",
            None,
        );
        let same_clone_impl = rvs_impl_identity_marker(
            "T",
            "",
            Some(("app::Specialized", "trait:local")),
            "T: app::Clone",
            "trait:local",
            None,
        );
        let copy_impl = rvs_impl_identity_marker(
            "T",
            "",
            Some(("app::Specialized", "trait:local")),
            "T: app::Copy",
            "trait:local",
            None,
        );
        let first_generated = rvs_impl_identity_marker(
            "[T; ($n - 1)]",
            "",
            Some(("core::default::Default", "trait:0000000000000001")),
            "T: core::default::Default",
            "trait:0000000000000001",
            Some("first"),
        );
        let second_generated = rvs_impl_identity_marker(
            "[T; ($n - 1)]",
            "",
            Some(("core::default::Default", "trait:0000000000000001")),
            "T: core::default::Default",
            "trait:0000000000000001",
            Some("second"),
        );
        let output = format!(
            "same_predicates_stable={}\nspecialization_predicates_distinct={}\ngenerated_impls_distinct={}\n",
            clone_impl == same_clone_impl,
            clone_impl != copy_impl,
            first_generated != second_generated,
        );
        rvs_snapshot_BIS(
            "test_20260731_impl_marker_distinguishes_specialization_predicates",
            &output,
        );

        assert_eq!(clone_impl, same_clone_impl);
        assert_ne!(clone_impl, copy_impl);
        assert_ne!(first_generated, second_generated);
    }

    #[test]
    fn test_20260630_utils_helper_coverage() {
        assert!(rvs_valid_test("test_20260630_utils_helper_coverage"));
        rvs_register_test_coverage((
            rvs_has_attr,
            rvs_has_allow,
            rvs_allows_non_snake_case,
            rvs_has_doc_section,
            rvs_has_any_doc,
            rvs_has_debug_derive,
            rvs_has_mutable_params,
            rvs_is_empty_body,
            rvs_collect_local_bindings_M,
            rvs_static_is_thread_local,
            rvs_count_effective_lines_M,
            rvs_root_body_expr,
            rvs_qp,
            rvs_tys,
            rvs_def_path,
            rvs_resolve_call,
            rvs_is_sysroot_crate_id,
        ));
    }
}
