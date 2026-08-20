use rustc_hir::OwnerId;
use rustc_hir::def::Res;
use rustc_hir::{
    self, Body, Expr, ExprKind, ImplicitSelfKind, Item, ItemKind, Mutability, OwnerNode, QPath,
    VariantData,
};
use rustc_lint::LateContext;
use rustc_middle::ty::Visibility;
use rustc_span::def_id::DefId;
use rustc_span::kw;

use super::super::RVS_DATA_STRUCT_MUT_METHOD;
use super::super::RVS_REDUNDANT_FIELD_ACCESSOR;
use super::super::msg::rvs_emit_span_lint_S;

/// Returns `true` when the field is explicitly annotated with a crate-wide
/// visibility: `pub`, `pub(crate)`, or `pub(in crate)`.
///
/// The ty query normalizes an unannotated field at the crate root to the same
/// `Restricted(CRATE_DEF_ID)` value as an explicit `pub(crate)`, and
/// `pub(self)`/`pub(super)` at the crate root to the same value as well; only
/// the source annotation distinguishes intent.
fn rvs_field_is_data_public(cx: &LateContext<'_>, field: &rustc_hir::FieldDef<'_>) -> bool {
    if field.vis_span.is_empty() {
        return false;
    }
    let crate_wide = cx
        .tcx
        .visibility(field.def_id.to_def_id())
        .is_accessible_from(rustc_span::def_id::CRATE_DEF_ID.to_def_id(), cx.tcx);
    if !crate_wide {
        return false;
    }
    let Ok(snippet) = cx.tcx.sess.source_map().span_to_snippet(field.vis_span) else {
        return false;
    };
    rvs_annotation_is_crate_wide(&snippet)
}

/// Returns `true` when the visibility annotation is one of the crate-wide
/// forms; module-local spellings (`pub(self)`, `pub(super)`) are rejected even
/// when they resolve to the crate root. Whitespace and comments inside the
/// annotation are ignored: `pub (crate)` and `pub(in crate)` count.
fn rvs_annotation_is_crate_wide(snippet: &str) -> bool {
    let Some(tokens) = rvs_annotation_tokens(snippet) else {
        return false;
    };
    const FORMS: [&[&str]; 3] = [
        &["pub"],
        &["pub", "(", "crate", ")"],
        &["pub", "(", "in", "crate", ")"],
    ];
    FORMS.iter().any(|form| tokens == *form)
}

/// Tokenizes a visibility annotation into identifier/punctuation strings,
/// skipping whitespace and comments; `None` on lexing failure.
fn rvs_annotation_tokens(snippet: &str) -> Option<Vec<&str>> {
    let mut tokens = Vec::new();
    let mut offset = 0usize;
    for token in rustc_lexer::tokenize(snippet, rustc_lexer::FrontmatterAllowed::No) {
        let Ok(token_len) = usize::try_from(token.len) else {
            return None;
        };
        let Some(end) = offset.checked_add(token_len) else {
            return None;
        };
        let text = snippet.get(offset..end)?;
        match token.kind {
            rustc_lexer::TokenKind::LineComment { .. }
            | rustc_lexer::TokenKind::BlockComment { .. }
            | rustc_lexer::TokenKind::Whitespace => {}
            rustc_lexer::TokenKind::Ident
            | rustc_lexer::TokenKind::OpenParen
            | rustc_lexer::TokenKind::CloseParen => {
                tokens.push(text);
            }
            _ => return None,
        }
        offset = end;
    }
    Some(tokens)
}

/// Returns `true` when the ADT is a non-empty struct whose every field is
/// explicitly annotated with a crate-wide visibility.
///
/// Such a type is classified as pure data: field access and construction are
/// part of its public surface, so behavior belongs in free functions.
pub(crate) fn rvs_is_public_fields_data_S(cx: &LateContext<'_>, adt_def_id: DefId) -> bool {
    let adt = cx.tcx.adt_def(adt_def_id);
    if !adt.is_struct() {
        return false;
    }
    let variant = adt.non_enum_variant();
    // Empty structs have no public field surface; classifying them as data by
    // vacuous truth would flag stateless objects as pure data.
    if variant.fields.is_empty() {
        return false;
    }
    let OwnerNode::Item(item) = cx.tcx.hir_owner_node(OwnerId {
        def_id: adt_def_id.expect_local(),
    }) else {
        return false;
    };
    let ItemKind::Struct(_, _, data) = &item.kind else {
        return false;
    };
    match data {
        VariantData::Struct { fields, .. } | VariantData::Tuple(fields, _, _) => fields
            .iter()
            .all(|field| rvs_field_is_data_public(cx, field)),
        VariantData::Unit(..) => false,
    }
}

/// Returns the local ADT `DefId` of an inherent impl block's self type when
/// it directly implements a nominal struct; `None` for trait impls,
/// non-nominal self types, and non-local types.
pub(crate) fn rvs_inherent_struct_def_id<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    imp: &'tcx rustc_hir::Impl<'tcx>,
) -> Option<DefId> {
    if imp.of_trait.is_some() {
        return None;
    }
    debug_assert!(
        matches!(&item.kind, ItemKind::Impl(inner) if std::ptr::eq(inner, imp)),
        "impl item and impl kind must refer to the same node"
    );
    let self_ty = cx.tcx.type_of(item.owner_id.def_id).skip_binder();
    let rustc_middle::ty::TyKind::Adt(adt_def, _) = self_ty.kind() else {
        return None;
    };
    let def_id = adt_def.did();
    def_id.is_local().then_some(def_id)
}

/// How a method receives `self`. By-value receivers consume the struct and
/// never count as `&mut self`; non-receiver functions have `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverKind {
    /// `self`, `mut self`, or `self: Self`.
    Value,
    /// `&self` or `self: &Self`.
    Ref,
    /// `&mut self` or `self: &mut Self`.
    RefMut,
    /// No receiver.
    None,
}

/// Classifies the receiver of a method signature, covering implicit and
/// explicit (`self: &Self`) spellings.
pub(crate) fn rvs_receiver_kind(sig: &rustc_hir::FnSig<'_>, body: &Body<'_>) -> ReceiverKind {
    match sig.decl.implicit_self {
        ImplicitSelfKind::RefImm => return ReceiverKind::Ref,
        ImplicitSelfKind::RefMut => return ReceiverKind::RefMut,
        ImplicitSelfKind::Imm | ImplicitSelfKind::Mut => return ReceiverKind::Value,
        ImplicitSelfKind::None => {}
    }
    let is_self_param = matches!(
        body.params.first().map(|param| &param.pat.kind),
        Some(rustc_hir::PatKind::Binding(_, _, ident, _)) if ident.name == kw::SelfLower
    );
    if !is_self_param {
        return ReceiverKind::None;
    }
    match sig.decl.inputs.first().map(|input| &input.kind) {
        Some(rustc_hir::TyKind::Ref(_, rustc_hir::MutTy { mutbl, .. })) => match mutbl {
            Mutability::Not => ReceiverKind::Ref,
            Mutability::Mut => ReceiverKind::RefMut,
        },
        Some(_) => ReceiverKind::Value,
        None => ReceiverKind::None,
    }
}

/// Returns `true` when the field's visibility dominates the method's
/// effective visibility, i.e. every caller that can invoke the method can
/// also read the field directly.
///
/// The effective visibility intersects the method's declared visibility with
/// the enclosing struct's and parent modules' visibilities, including
/// `pub use` re-exports; a `pub` method in a private, non-re-exported module
/// does not actually widen access.
fn rvs_field_dominates_method_S(
    cx: &LateContext<'_>,
    field_def_id: DefId,
    method_def_id: DefId,
) -> bool {
    let declared = cx.tcx.visibility(method_def_id);
    let method_local = method_def_id.as_local();
    let effective_vis: Visibility<DefId> = match method_local {
        Some(local) => cx
            .tcx
            .effective_visibilities(())
            .effective_vis(local)
            .map(|effective| effective.at_level(rustc_middle::middle::privacy::Level::Reexported))
            .copied()
            .map(Visibility::to_def_id)
            .unwrap_or(declared),
        None => declared,
    };
    cx.tcx
        .visibility(field_def_id)
        .is_at_least(effective_vis, cx.tcx)
}

/// Check an inherent method of a pure-data struct: forbid `&mut self`
/// receivers and bodies that only project a field whose visibility dominates
/// the method's effective visibility (declaration visibility intersected
/// with the enclosing struct/module chain).
pub(crate) fn rvs_check_data_method_S<'tcx>(
    cx: &LateContext<'tcx>,
    sig: &'tcx rustc_hir::FnSig<'tcx>,
    body: &'tcx Body<'tcx>,
    method_def_id: DefId,
    struct_name: &str,
    method_name: &str,
    adt_def_id: DefId,
) {
    match rvs_receiver_kind(sig, body) {
        ReceiverKind::RefMut => {
            rvs_emit_span_lint_S(
                cx,
                RVS_DATA_STRUCT_MUT_METHOD,
                sig.span,
                format!(
                    "{struct_name} is pure data (all fields visible); &mut self method \
                     '{method_name}' — mutate via free functions or hide the fields"
                ),
            );
        }
        ReceiverKind::Ref | ReceiverKind::Value => {
            if let Some((field_name, field_def_id)) =
                rvs_direct_field_projection_S(cx, body, adt_def_id)
                && rvs_field_dominates_method_S(cx, field_def_id, method_def_id)
            {
                rvs_emit_span_lint_S(
                    cx,
                    RVS_REDUNDANT_FIELD_ACCESSOR,
                    sig.span,
                    format!(
                        "method '{method_name}' only returns field '{field_name}' of pure-data \
                         struct '{struct_name}', which is already visible to every caller — \
                         remove the accessor or make the field private"
                    ),
                );
            }
        }
        ReceiverKind::None => {}
    }
}

/// Returns the projected field name and its `DefId` when the body is exactly
/// a (possibly borrowed, block-wrapped, or early-returned) `self.field`
/// projection naming a declared field of the struct; any other statement or
/// computation disqualifies the method.
pub(crate) fn rvs_direct_field_projection_S<'tcx>(
    cx: &LateContext<'tcx>,
    body: &'tcx Body<'tcx>,
    adt_def_id: DefId,
) -> Option<(&'tcx str, DefId)> {
    let self_binding = rvs_self_binding_id_S(body)?;
    let mut current = rvs_root_body_expr(body);
    loop {
        match current.kind {
            ExprKind::Block(block, _) => {
                if block.stmts.is_empty() {
                    let Some(tail) = block.expr else { return None };
                    current = tail;
                } else if let [stmt] = block.stmts
                    && block.expr.is_none()
                    && let rustc_hir::StmtKind::Semi(expr) = stmt.kind
                    // A single `return self.field;` statement is equivalent
                    // to a tail projection.
                    && let ExprKind::Ret(Some(inner)) = expr.kind
                {
                    current = inner;
                } else {
                    return None;
                }
            }
            // A tail `return self.field` (no semicolon) is equivalent too.
            ExprKind::Ret(Some(inner)) => current = inner,
            ExprKind::Use(inner, _) => current = inner,
            _ => break,
        }
    }
    loop {
        match current.kind {
            ExprKind::AddrOf(_, _, inner) => current = inner,
            ExprKind::Field(base, ident) => {
                return if rvs_is_self_path(base, self_binding) {
                    rvs_declared_field(cx, ident, adt_def_id)
                } else {
                    None
                };
            }
            _ => return None,
        }
    }
}

/// Returns the `HirId` of the implicit `self` binding of a method body.
fn rvs_self_binding_id_S(body: &Body<'_>) -> Option<rustc_hir::HirId> {
    let param = body.params.first()?;
    match param.pat.kind {
        rustc_hir::PatKind::Binding(_, hir_id, ident, _) if ident.name == kw::SelfLower => {
            Some(hir_id)
        }
        _ => None,
    }
}

fn rvs_declared_field<'tcx>(
    cx: &LateContext<'tcx>,
    ident: rustc_span::Ident,
    adt_def_id: DefId,
) -> Option<(&'tcx str, DefId)> {
    let variant = cx.tcx.adt_def(adt_def_id).non_enum_variant();
    variant
        .fields
        .iter()
        .find(|field| field.name == ident.name)
        .map(|field| (field.name.as_str(), field.did))
}

fn rvs_is_self_path(expr: &Expr<'_>, self_binding: rustc_hir::HirId) -> bool {
    match expr.kind {
        ExprKind::Path(QPath::Resolved(
            _,
            rustc_hir::Path {
                res: Res::Local(hir_id),
                ..
            },
        )) => *hir_id == self_binding,
        _ => false,
    }
}

const fn rvs_root_body_expr<'hir>(body: &'hir Body<'hir>) -> &'hir Expr<'hir> {
    body.value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};

    #[test]
    fn test_20260819_data_struct_ui_coverage() {
        rvs_snapshot_BIS("test_20260819_data_struct_ui_coverage", "covered\n");
        rvs_register_test_coverage(rvs_is_public_fields_data_S);
        rvs_register_test_coverage(rvs_inherent_struct_def_id);
        rvs_register_test_coverage(rvs_check_data_method_S);
        rvs_register_test_coverage(rvs_direct_field_projection_S);
        rvs_register_test_coverage(rvs_receiver_kind);
    }

    #[test]
    fn test_20260819_data_struct_annotation_forms() {
        let cases = [
            ("pub", true),
            ("pub(crate)", true),
            ("pub(in crate)", true),
            ("pub (crate)", true),
            ("pub(\n    crate\n)", true),
            ("pub(in /* crate root */ crate)", true),
            ("  pub  ", true),
            ("pub(self)", false),
            ("pub(super)", false),
            ("pub(in crate::inner)", false),
            ("", false),
            ("pub(in", false),
        ];
        let mut output = String::new();
        for (annotation, expected) in cases {
            let actual = rvs_annotation_is_crate_wide(annotation);
            output.push_str(&format!("{annotation:?}={actual}\n"));
            assert_eq!(actual, expected, "annotation {annotation:?}");
        }
        rvs_snapshot_BIS("test_20260819_data_struct_annotation_forms", &output);
    }
}
