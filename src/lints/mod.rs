#![allow(clippy::all)]
#![allow(internal_features)]

use std::collections::{BTreeMap, HashSet};

use rustc_lint::{LateContext, LateLintPass, LintPass};
use rustc_session::declare_tool_lint;
use rustc_span::Span;

use crate::capsmap::CapsMap;

mod banned_import;
mod borrowed_param;
mod call_violation;
mod callgraph;
mod catch_all_error;
mod catch_unwind;
mod consumed_arg;
mod ctx;
mod dead_code;
mod debug_assert;
mod deny_warnings;
mod deref_polymorphism;
mod empty_fn;
mod error_swallow;
mod missing_allow;
mod missing_debug_derive;
mod missing_doc;
mod missing_safety_doc;
mod msg;
mod non_rvs_fn;
mod port_traits;
mod reflection;
mod signature_caps;
mod spawn;
mod static_ref;
mod stub_macro;
mod suffix_order;
mod test_name_format;
mod test_quality;
mod todo_comment;
mod utils;
mod validate;

pub use callgraph::FnBehavior;

use callgraph::FnReportEntry;
use ctx::FnCheckData;

// ─── Lint declarations ───────────────────────────────────────────────────

macro_rules! rvs_declare {
    ($name:ident, Deny, $desc:expr) => {
        declare_tool_lint! { pub rivus::$name, Deny, $desc }
    };
    ($name:ident, Warn, $desc:expr) => {
        declare_tool_lint! { pub rivus::$name, Warn, $desc }
    };
}

rvs_declare!(RVS_CALL_VIOLATION, Deny, "capability call chain violation");
rvs_declare!(
    RVS_STATIC_REF,
    Deny,
    "static/thread_local reference without capability"
);
rvs_declare!(RVS_STUB_MACRO, Deny, "todo!/unimplemented!() stub");
rvs_declare!(RVS_EMPTY_FN, Deny, "empty function body");
rvs_declare!(
    RVS_MISSING_DEBUG_ASSERT,
    Warn,
    "primitive numeric parameter without debug_assert!"
);
rvs_declare!(
    RVS_MISSING_ALLOW,
    Warn,
    "rvs_ function with uppercase suffix but no #[allow(non_snake_case)]"
);
rvs_declare!(RVS_NON_RVS_FN, Warn, "function missing rvs_ prefix");
rvs_declare!(
    RVS_UNKNOWN_CALLEE,
    Warn,
    "call to function neither rvs_-prefixed nor in capsmap"
);
rvs_declare!(
    RVS_MISSING_MUTABLE,
    Warn,
    "function has &mut param but suffix lacks M"
);
rvs_declare!(RVS_MISSING_ASYNC, Warn, "async fn but suffix lacks A");
rvs_declare!(RVS_MISSING_UNSAFE, Warn, "unsafe code but suffix lacks U");
rvs_declare!(
    RVS_MISSING_SIDE_EFFECT,
    Warn,
    "reads static but suffix lacks S"
);
rvs_declare!(
    RVS_MISSING_THREAD_LOCAL,
    Warn,
    "reads thread_local! but suffix lacks T"
);
rvs_declare!(
    RVS_NON_ALPHABETICAL_SUFFIX,
    Warn,
    "capability suffix letters not in alphabetical order"
);
rvs_declare!(
    RVS_DUPLICATE_SUFFIX,
    Warn,
    "duplicate letter in capability suffix"
);
rvs_declare!(
    RVS_UNKNOWN_SUFFIX_LETTER,
    Warn,
    "suffix contains unrecognized capability letters"
);
rvs_declare!(RVS_SPAWN_WARNING, Warn, "unstructured spawn");
rvs_declare!(
    RVS_DEAD_CODE,
    Warn,
    "rvs_ function marked #[allow(dead_code)] or #[allow(unused)]"
);
rvs_declare!(
    RVS_TEST_NAME_FORMAT,
    Warn,
    "test name does not match format"
);
rvs_declare!(RVS_BANNED_IMPORT, Warn, "import of banned crate");
rvs_declare!(
    RVS_MISSING_DEBUG_DERIVE,
    Warn,
    "pub struct/enum missing #[derive(Debug)]"
);
rvs_declare!(
    RVS_ERROR_SWALLOW,
    Warn,
    ".ok() or .unwrap_or_default() swallows errors"
);
rvs_declare!(
    RVS_CATCH_UNWIND,
    Warn,
    "catch_unwind — fix panic source instead"
);
rvs_declare!(
    RVS_REFLECTION_USAGE,
    Warn,
    "std::any::Any/type_name/type_id — use trait dispatch"
);
rvs_declare!(
    RVS_BORROWED_PARAM,
    Warn,
    "&String/&Vec<T>/&Box<T> — use &str/&[T]/&T"
);
rvs_declare!(RVS_INTO_IMPL, Warn, "impl Into<T> — implement From instead");
rvs_declare!(
    RVS_DEREF_POLYMORPHISM,
    Warn,
    "impl Deref — use composition instead"
);
rvs_declare!(
    RVS_DENY_WARNINGS,
    Warn,
    "#![deny(warnings)] — use named lints"
);
rvs_declare!(RVS_WILDCARD_IMPORT, Warn, "use xxx::*; wildcard import");
rvs_declare!(
    RVS_MISSING_DOC,
    Warn,
    "pub fn/method missing /// doc comment"
);
rvs_declare!(
    RVS_MISSING_SAFETY_DOC,
    Warn,
    "unsafe fn missing /// # Safety"
);
rvs_declare!(
    RVS_CATCH_ALL_ERROR_VARIANT,
    Warn,
    "error enum has Unknown/Other catch-all variant"
);
rvs_declare!(
    RVS_VALIDATE_RETURNS_UNIT,
    Warn,
    "validate/check/verify returns Result<(),E> — use TryFrom"
);
rvs_declare!(
    RVS_CONSUMED_ARG_ON_ERROR,
    Warn,
    "owned param consumed but not preserved in error type"
);
rvs_declare!(RVS_TODO_COMMENT, Warn, "// TODO or // FIXME comment");
rvs_declare!(
    RVS_MISSING_TEST_OUTPUT,
    Warn,
    "test missing test_out/{name}.out snapshot"
);
rvs_declare!(RVS_DUPLICATE_TEST, Warn, "duplicate test function name");
rvs_declare!(
    RVS_UNTESTED_GOOD_FN,
    Warn,
    "good function not called by any test"
);
rvs_declare!(
    RVS_UNTESTED_OK_FN,
    Warn,
    "ok function (ABMP subset, mock-testable) not called by any test"
);

pub static RIVUS_LINTS: &[&rustc_lint::Lint] = &[
    RVS_CALL_VIOLATION,
    RVS_STATIC_REF,
    RVS_STUB_MACRO,
    RVS_EMPTY_FN,
    RVS_MISSING_DEBUG_ASSERT,
    RVS_MISSING_ALLOW,
    RVS_NON_RVS_FN,
    RVS_UNKNOWN_CALLEE,
    RVS_MISSING_MUTABLE,
    RVS_MISSING_ASYNC,
    RVS_MISSING_UNSAFE,
    RVS_MISSING_SIDE_EFFECT,
    RVS_MISSING_THREAD_LOCAL,
    RVS_NON_ALPHABETICAL_SUFFIX,
    RVS_DUPLICATE_SUFFIX,
    RVS_UNKNOWN_SUFFIX_LETTER,
    RVS_SPAWN_WARNING,
    RVS_DEAD_CODE,
    RVS_TEST_NAME_FORMAT,
    RVS_BANNED_IMPORT,
    RVS_MISSING_DEBUG_DERIVE,
    RVS_ERROR_SWALLOW,
    RVS_CATCH_UNWIND,
    RVS_REFLECTION_USAGE,
    RVS_BORROWED_PARAM,
    RVS_INTO_IMPL,
    RVS_DEREF_POLYMORPHISM,
    RVS_DENY_WARNINGS,
    RVS_WILDCARD_IMPORT,
    RVS_MISSING_DOC,
    RVS_MISSING_SAFETY_DOC,
    RVS_CATCH_ALL_ERROR_VARIANT,
    RVS_VALIDATE_RETURNS_UNIT,
    RVS_CONSUMED_ARG_ON_ERROR,
    RVS_TODO_COMMENT,
    RVS_MISSING_TEST_OUTPUT,
    RVS_DUPLICATE_TEST,
    RVS_UNTESTED_GOOD_FN,
    RVS_UNTESTED_OK_FN,
];

// ─── Lint pass ───────────────────────────────────────────────────────────

pub struct RivusLintPass {
    capsmap: Option<CapsMap>,
    test_names: BTreeMap<String, Vec<Span>>,
    good_fns: Vec<(String, Span)>,
    ok_fns: Vec<(String, Span)>,
    test_call_names: HashSet<String>,
    fn_report: Vec<FnReportEntry>,
    callgraph: BTreeMap<String, FnBehavior>,
    done_crate_level: bool,
    collect_callgraph: bool,
    emit_report: bool,
    should_emit_lints: bool,
    test_fn_names: HashSet<String>,
    /// DefIds of Port traits (names ending in Repository/Client) in this crate.
    port_traits: HashSet<rustc_span::def_id::DefId>,
}

impl RivusLintPass {
    pub fn new() -> Self {
        Self {
            capsmap: None,
            test_names: BTreeMap::new(),
            good_fns: Vec::new(),
            ok_fns: Vec::new(),
            test_call_names: HashSet::new(),
            fn_report: Vec::new(),
            callgraph: BTreeMap::new(),
            done_crate_level: false,
            collect_callgraph: std::env::var("RIVUS_CALLGRAPH").is_ok(),
            emit_report: std::env::var("RIVUS_REPORT").is_ok(),
            should_emit_lints: !std::env::var("RIVUS_CALLGRAPH").is_ok(),
            test_fn_names: HashSet::new(),
            port_traits: HashSet::new(),
        }
    }

    fn rvs_ensure_capsmap_BIMS(&mut self) {
        if self.capsmap.is_some() {
            return;
        }
        if let Ok(path_str) = std::env::var("RIVUS_CAPSMAP") {
            let path = std::path::PathBuf::from(&path_str);
            self.capsmap = Some(match CapsMap::rvs_load_BIS(&path) {
                Ok(cm) => cm,
                Err(e) => {
                    eprintln!("warning: {}: {e}", path.display());
                    CapsMap::rvs_new()
                }
            });
        } else {
            self.capsmap = Some(CapsMap::rvs_new());
        }
    }
}

impl Default for RivusLintPass {
    fn default() -> Self {
        Self::new()
    }
}

impl LintPass for RivusLintPass {
    fn name(&self) -> &'static str {
        "RivusLintPass"
    }
    fn get_lints(&self) -> Vec<&'static rustc_lint::Lint> {
        RIVUS_LINTS.to_vec()
    }
}

impl<'tcx> LateLintPass<'tcx> for RivusLintPass {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        self.rvs_ensure_capsmap_BIMS();

        // Collect Port traits (names ending in Repository/Client) in this crate.
        self.port_traits = port_traits::rvs_collect_port_traits_S(cx);

        // Pre-scan: collect names of test functions
        if cx.tcx.sess.is_test_crate() {
            let krate = cx.tcx.hir_crate_items(());
            for owner in krate.owners() {
                let hir_id = rustc_hir::HirId::from(owner);
                let attrs = cx.tcx.hir_attrs(hir_id);
                for a in attrs {
                    if let rustc_hir::Attribute::Parsed(
                        rustc_hir::attrs::AttributeKind::RustcTestMarker(_),
                    ) = a
                    {
                        let node = cx.tcx.hir_node_by_def_id(owner.def_id);
                        if let rustc_hir::Node::Item(item) = node {
                            if let rustc_hir::ItemKind::Const(ct, ..) = &item.kind {
                                self.test_fn_names.insert(ct.name.as_str().to_string());
                            }
                        }
                    }
                }
            }
        }

        if self.should_emit_lints {
            deny_warnings::rvs_check_crate_S(cx);
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        if self.done_crate_level {
            return;
        }
        self.done_crate_level = true;

        test_quality::rvs_check_crate_post_MS(
            cx,
            &self.test_names,
            &self.good_fns,
            &self.ok_fns,
            &self.test_call_names,
            &self.fn_report,
            &self.callgraph,
            self.emit_report,
            self.collect_callgraph,
        );
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::Item<'tcx>) {
        let mut data = FnCheckData {
            capsmap: &self.capsmap,
            good_fns: &mut self.good_fns,
            ok_fns: &mut self.ok_fns,
            fn_report: &mut self.fn_report,
            callgraph: &mut self.callgraph,
            collect_callgraph: self.collect_callgraph,
            should_emit_lints: self.should_emit_lints,
            port_traits: &self.port_traits,
        };
        rvs_check_item(
            cx,
            item,
            &self.test_fn_names,
            &mut self.test_names,
            &mut self.test_call_names,
            &mut data,
        );
    }

    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        impl_item: &'tcx rustc_hir::ImplItem<'tcx>,
    ) {
        let mut data = FnCheckData {
            capsmap: &self.capsmap,
            good_fns: &mut self.good_fns,
            ok_fns: &mut self.ok_fns,
            fn_report: &mut self.fn_report,
            callgraph: &mut self.callgraph,
            collect_callgraph: self.collect_callgraph,
            should_emit_lints: self.should_emit_lints,
            port_traits: &self.port_traits,
        };
        rvs_check_impl_item(
            cx,
            impl_item,
            &self.test_fn_names,
            &mut self.test_names,
            &mut self.test_call_names,
            &mut data,
        );
    }

    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx rustc_hir::TraitItem<'tcx>,
    ) {
        let mut data = FnCheckData {
            capsmap: &self.capsmap,
            good_fns: &mut self.good_fns,
            ok_fns: &mut self.ok_fns,
            fn_report: &mut self.fn_report,
            callgraph: &mut self.callgraph,
            collect_callgraph: self.collect_callgraph,
            should_emit_lints: self.should_emit_lints,
            port_traits: &self.port_traits,
        };
        rvs_check_trait_item(cx, trait_item, &mut data);
    }
}

// ─── Dispatch functions ──────────────────────────────────────────────────

/// Dispatches to fn-level checks for free functions, inherent impl methods,
/// and trait impl methods.
#[allow(clippy::too_many_arguments)]
fn rvs_run_fn_checks_MS<'tcx>(
    cx: &LateContext<'tcx>,
    name: &str,
    hir_id: rustc_hir::HirId,
    span: Span,
    sig: &rustc_hir::FnSig<'tcx>,
    body: &rustc_hir::Body<'tcx>,
    has_body: bool,
    is_test: bool,
    is_trait_impl_method: bool,
    is_port_method: bool,
    data: &mut FnCheckData<'_>,
) {
    let attrs = cx.tcx.hir_attrs(hir_id);

    if let Some(mut info) = utils::FnInfo::rvs_extract(name, sig, body, cx.tcx) {
        // Port trait methods get P capability automatically.
        if is_port_method {
            info.caps.rvs_insert_M(crate::capability::Capability::P);
        }

        let is_stub = stub_macro::rvs_check_fn_MS(cx, body, span);
        empty_fn::rvs_check_fn_MS(cx, body, span, has_body, is_stub);
        missing_allow::rvs_check_fn_S(cx, hir_id, span, &info.raw_suffix);
        dead_code::rvs_check_fn_S(cx, attrs, span);
        signature_caps::rvs_check_fn_S(cx, span, &info);
        suffix_order::rvs_check_fn_S(cx, span, &info.raw_suffix);

        // Body-level checks
        call_violation::rvs_check_fn_MS(cx, body, &info.caps, data.capsmap);

        // Spawn, reflection, catch_unwind, error swallow detection
        spawn::rvs_check_fn_MS(cx, body, is_test);
        reflection::rvs_check_fn_MS(cx, body);
        catch_unwind::rvs_check_fn_MS(cx, body);
        error_swallow::rvs_check_fn_MS(cx, body);

        if has_body && !is_stub {
            static_ref::rvs_check_fn_MS(cx, body, &info.caps);
            debug_assert::rvs_check_fn_MS(cx, body);
            borrowed_param::rvs_check_fn_params_S(cx, sig);
            consumed_arg::rvs_check_fn_MS(cx, sig, name);
            validate::rvs_check_fn_S(cx, name, sig);
        }

        // Collect good fns for later untested-good-fn check
        let good = crate::capability::CapabilitySet::rvs_from_good_caps();
        if info.caps.rvs_is_subset_of(&good)
            && !is_test
            && !utils::rvs_has_allow(attrs, "dead_code")
            && !utils::rvs_has_allow(attrs, "unused")
        {
            data.good_fns.push((name.to_string(), span));
        }

        // Collect ok fns (ABMP subset, mock-testable) for untested-ok-fn check.
        let ok = crate::capability::CapabilitySet::rvs_from_ok_caps();
        if info.caps.rvs_is_subset_of(&ok)
            && !is_test
            && !utils::rvs_has_allow(attrs, "dead_code")
            && !utils::rvs_has_allow(attrs, "unused")
        {
            data.ok_fns.push((name.to_string(), span));
        }

        // Collect fn report entry
        let allows_dead_code =
            utils::rvs_has_allow(attrs, "dead_code") || utils::rvs_has_allow(attrs, "unused");
        let effective_lines = if has_body {
            utils::rvs_count_effective_lines_M(cx, body)
        } else {
            0
        };
        let caps_str: String = info.caps.rvs_iter().map(|c| c.rvs_as_char()).collect();
        data.fn_report.push(FnReportEntry {
            name: name.to_string(),
            caps: caps_str,
            lines: effective_lines,
            is_test,
            allows_dead_code,
        });
    } else if !is_test
        && name != "main"
        && name != "new"
        && name != "go"
        && name != "wblk"
        && !is_trait_impl_method
    {
        non_rvs_fn::rvs_check_fn_S(cx, name, span);
    }
    test_name_format::rvs_check_fn_S(cx, name, span, is_test);
}

/// Check free-fn / struct / enum / use / impl items.
#[allow(clippy::too_many_arguments)]
fn rvs_check_item<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx rustc_hir::Item<'tcx>,
    test_fn_names: &HashSet<String>,
    test_names: &mut BTreeMap<String, Vec<Span>>,
    test_call_names: &mut HashSet<String>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{ItemKind, VariantData};

    match &item.kind {
        ItemKind::Fn {
            sig,
            body,
            ident,
            has_body,
            ..
        } => {
            let name = ident.name.as_str();
            let body = cx.tcx.hir_body(*body);
            let attrs = cx.tcx.hir_attrs(item.hir_id());
            let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
            if data.should_emit_lints {
                rvs_run_fn_checks_MS(
                    cx,
                    name,
                    item.hir_id(),
                    item.span,
                    sig,
                    body,
                    *has_body,
                    is_test,
                    false,
                    false,
                    data,
                );
                test_names
                    .entry(name.to_string())
                    .or_default()
                    .push(item.span);
                if is_test {
                    utils::rvs_collect_test_call_names_M(cx.tcx, body, test_call_names);
                }
                let vis = cx.tcx.visibility(item.owner_id.def_id);
                let is_pub = vis.is_public();
                missing_doc::rvs_check_fn_S(cx, name, item.span, attrs, is_pub);
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    item.hir_id(),
                    item.span,
                    &sig.header.safety,
                );
                todo_comment::rvs_check_fn_S(cx, item.span);
            }
            if data.collect_callgraph {
                callgraph::rvs_collect_callgraph_for_item_M(
                    data.callgraph,
                    cx,
                    item.hir_id(),
                    sig,
                    body,
                    false,
                    is_test,
                    false,
                );
            }
        }
        ItemKind::Use(path, use_kind) => {
            if data.should_emit_lints {
                banned_import::rvs_check_item_S(cx, item, path, *use_kind);
            }
        }
        ItemKind::Enum(_, _, enum_def) => {
            if data.should_emit_lints {
                missing_debug_derive::rvs_check_struct_or_enum_S(cx, item);
                catch_all_error::rvs_check_enum_S(cx, item, enum_def);
            }
        }
        ItemKind::Struct(_, _, data_fields) => {
            if data.should_emit_lints {
                missing_debug_derive::rvs_check_struct_or_enum_S(cx, item);
                if let VariantData::Struct { fields, .. } = data_fields {
                    borrowed_param::rvs_check_borrowed_fields_S(cx, fields);
                }
            }
        }
        ItemKind::Impl(imp) => {
            if data.should_emit_lints {
                deref_polymorphism::rvs_check_impl_S(cx, item, imp);
            }
        }
        _ => {}
    }
}

/// Check inherent impl method.
#[allow(clippy::too_many_arguments)]
fn rvs_check_impl_item<'tcx>(
    cx: &LateContext<'tcx>,
    impl_item: &'tcx rustc_hir::ImplItem<'tcx>,
    test_fn_names: &HashSet<String>,
    test_names: &mut BTreeMap<String, Vec<Span>>,
    test_call_names: &mut HashSet<String>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{Item, ItemKind};

    if let rustc_hir::ImplItemKind::Fn(sig, body_id) = &impl_item.kind {
        let parent = cx.tcx.hir_get_parent_item(impl_item.hir_id());
        let parent_node = cx.tcx.hir_owner_node(parent);
        let is_trait_impl = matches!(
            parent_node,
            rustc_hir::OwnerNode::Item(Item {
                kind: ItemKind::Impl(rustc_hir::Impl {
                    of_trait: Some(_),
                    ..
                }),
                ..
            })
        );
        // Check if this is a Port trait impl method
        let is_port_method = is_trait_impl && {
            if let rustc_hir::OwnerNode::Item(Item {
                kind: ItemKind::Impl(imp),
                ..
            }) = parent_node
            {
                if let Some(trait_ref) = &imp.of_trait
                    && let Some(trait_did) = trait_ref.trait_ref.trait_def_id()
                {
                    data.port_traits.contains(&trait_did)
                } else {
                    false
                }
            } else {
                false
            }
        };
        let name = impl_item.ident.name.as_str();
        let body = cx.tcx.hir_body(*body_id);
        let attrs = cx.tcx.hir_attrs(impl_item.hir_id());
        let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
        let is_pub = utils::rvs_is_pub_impl_item(cx, impl_item);
        // Port trait methods are checked (with P capability auto-assigned),
        // even though other trait impl methods are skipped.
        let should_check_fn = data.should_emit_lints && (!is_trait_impl || is_port_method);
        if should_check_fn {
            rvs_run_fn_checks_MS(
                cx,
                name,
                impl_item.hir_id(),
                impl_item.span,
                sig,
                body,
                true,
                is_test,
                is_trait_impl,
                is_port_method,
                data,
            );
            if is_test {
                test_names
                    .entry(name.to_string())
                    .or_default()
                    .push(impl_item.span);
                utils::rvs_collect_test_call_names_M(cx.tcx, body, test_call_names);
            }
            if !is_test && is_pub && !is_trait_impl {
                missing_doc::rvs_check_fn_S(cx, name, impl_item.span, attrs, true);
            }
            if is_pub && !is_trait_impl {
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    impl_item.hir_id(),
                    impl_item.span,
                    &sig.header.safety,
                );
            }
        }
        if data.collect_callgraph {
            callgraph::rvs_collect_callgraph_for_item_M(
                data.callgraph,
                cx,
                impl_item.hir_id(),
                sig,
                body,
                is_trait_impl,
                is_test,
                is_port_method,
            );
        }
    }
}

/// Check trait method (provided or required).
fn rvs_check_trait_item<'tcx>(
    cx: &LateContext<'tcx>,
    trait_item: &'tcx rustc_hir::TraitItem<'tcx>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{TraitFn, TraitItemKind};

    // Determine if this trait item belongs to a Port trait.
    let parent = cx.tcx.hir_get_parent_item(trait_item.hir_id());
    let parent_def_id = parent.def_id.to_def_id();
    let is_port_trait = data.port_traits.contains(&parent_def_id);

    match &trait_item.kind {
        TraitItemKind::Fn(sig, TraitFn::Provided(body_id)) => {
            let name = trait_item.ident.name.as_str();
            let body = cx.tcx.hir_body(*body_id);
            if data.should_emit_lints {
                rvs_run_fn_checks_MS(
                    cx,
                    name,
                    trait_item.hir_id(),
                    trait_item.span,
                    sig,
                    body,
                    true,
                    false,
                    true,
                    is_port_trait,
                    data,
                );
            }
            if data.collect_callgraph {
                callgraph::rvs_collect_callgraph_for_item_M(
                    data.callgraph,
                    cx,
                    trait_item.hir_id(),
                    sig,
                    body,
                    false,
                    false,
                    is_port_trait,
                );
            }
        }
        TraitItemKind::Fn(sig, TraitFn::Required(_)) => {
            // Required methods (no body) — collect signature info for callgraph.
            if data.collect_callgraph {
                callgraph::rvs_collect_callgraph_for_signature_M(
                    data.callgraph,
                    cx,
                    trait_item.hir_id(),
                    sig,
                    false,
                    is_port_trait,
                );
            }
        }
        _ => {}
    }
}
