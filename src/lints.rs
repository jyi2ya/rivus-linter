#![allow(clippy::all)]
#![allow(internal_features)]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use rustc_errors::{Diag, DiagCtxtHandle, Diagnostic, Level};
use rustc_hir::def::DefKind;
use rustc_hir::{
    Block, BlockCheckMode, Body, Expr, ExprKind, FnRetTy, GenericArg, HirId, Impl, ImplItem,
    ImplItemImplKind, ImplItemKind, Item, ItemKind, Mutability, PatKind, QPath, Safety, TraitFn,
    TraitItem, TraitItemKind, TyKind, UseKind, VariantData, attrs::AttributeKind, def::Res,
    def_id::DefId,
};
use rustc_lint::{LateContext, LateLintPass, LintContext, LintPass};
use rustc_span::{Span, Symbol};
use serde::Serialize;

use crate::capability::{
    Capability, CapabilitySet, rvs_extract_raw_suffix, rvs_extract_unknown_suffix_letters,
    rvs_parse_function,
};
use crate::capsmap::CapsMap;

#[derive(Debug, Serialize)]
struct FnReportEntry {
    name: String,
    caps: String,
    lines: usize,
    is_test: bool,
    allows_dead_code: bool,
}

// ─── Lint declarations ───────────────────────────────────────────────────

use rustc_session::declare_tool_lint;

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
];

// ─── Generic lint message ────────────────────────────────────────────────

#[derive(Debug)]
struct Msg {
    span: Span,
    text: String,
}

impl Msg {
    fn new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }
}

impl<'a> Diagnostic<'a, ()> for Msg {
    fn into_diag(self, dcx: DiagCtxtHandle<'a>, level: Level) -> Diag<'a, ()> {
        let mut d = Diag::new(dcx, level, format!("{}", self.text));
        d.span(self.span);
        d
    }
}

// ─── Constants ───────────────────────────────────────────────────────────

/// Spawn function def_paths — exact match only.
/// Use rustc's def_path (full path as seen in callgraph).
/// Short names shown here in comments for human reference.
const SPAWN_FUNCTIONS: &[&str] = &[
    // tokio::spawn
    "tokio::runtime::spawn",
    "tokio::task::spawn",
    "tokio::task::spawn_blocking",
    "tokio::task::spawn_local",
    // std::thread::spawn
    "std::thread::functions::spawn",
    "std::thread::builder::spawn",
    "std::thread::builder::spawn_unchecked",
    "std::thread::lifecycle::spawn_unchecked",
    // async_std::task::spawn
    "async_std::task::spawn",
    "async_std::task::spawn_blocking",
    // smol::spawn
    "smol::spawn",
];

/// Reflection def_paths — exact match only.
const REFLECTION_PATHS: &[&str] = &[
    "std::any::type_name",
    "std::any::type_id",
    "core::any::Any::type_id",
];

const ERROR_SWALLOW_METHODS: &[&str] = &["ok", "unwrap_or_default"];
const CATCH_ALL_VARIANT_NAMES: &[&str] = &["Unknown", "Other", "UnknownError", "OtherError"];
const VALIDATE_PREFIXES: &[&str] = &["validate", "check", "verify"];
const BORROWED_TYPES: &[&str] = &["String", "Vec", "Box"];

/// Exact match only — no suffix matching, no `format!` allocation.
fn rvs_is_spawn_S(path: &str) -> bool {
    SPAWN_FUNCTIONS.iter().any(|sf| *sf == path)
}

/// Exact match only.
fn rvs_is_reflection_S(path: &str) -> bool {
    REFLECTION_PATHS.iter().any(|rp| *rp == path)
}

// ─── Lint pass ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FnBehavior {
    pub calls: BTreeSet<String>,
    pub has_async: bool,
    pub has_unsafe_block: bool,
    pub is_unsafe_fn: bool,
    pub has_mut_param: bool,
    pub has_static_ref: bool,
    pub has_static_mut_ref: bool,
    pub has_thread_local_ref: bool,
    pub is_trait_impl: bool,
}

#[derive(Debug)]
pub struct RivusLintPass {
    capsmap: Option<CapsMap>,
    test_names: BTreeMap<String, Vec<Span>>,
    good_fns: Vec<(String, Span)>,
    test_call_names: HashSet<String>,
    fn_report: Vec<FnReportEntry>,
    callgraph: BTreeMap<String, FnBehavior>,
    done_crate_level: bool,
    collect_callgraph: bool,
    emit_report: bool,
    should_emit_lints: bool,
    /// Names of functions that are tests, detected via #[rustc_test_marker] consts
    /// generated by the #[test] macro.
    test_fn_names: HashSet<String>,
}

impl RivusLintPass {
    pub fn new() -> Self {
        Self {
            capsmap: None,
            test_names: BTreeMap::new(),
            good_fns: Vec::new(),
            test_call_names: HashSet::new(),
            fn_report: Vec::new(),
            callgraph: BTreeMap::new(),
            done_crate_level: false,
            collect_callgraph: std::env::var("RIVUS_CALLGRAPH").is_ok(),
            emit_report: std::env::var("RIVUS_REPORT").is_ok(),
            should_emit_lints: !std::env::var("RIVUS_CALLGRAPH").is_ok(),
            test_fn_names: HashSet::new(),
        }
    }

    fn rvs_ensure_capsmap_BIMS(&mut self) {
        if self.capsmap.is_some() {
            return;
        }
        if let Ok(path_str) = std::env::var("RIVUS_CAPSMAP") {
            let path = std::path::PathBuf::from(&path_str);
            self.capsmap = Some(if path.is_dir() {
                match CapsMap::rvs_load_from_dir_BIMS(&path) {
                    Ok(cm) => cm,
                    Err(e) => {
                        eprintln!("warning: caps/: {e}");
                        CapsMap::rvs_new()
                    }
                }
            } else {
                match std::fs::read_to_string(&path) {
                    Ok(c) => CapsMap::rvs_parse(&c).unwrap_or_else(|e| {
                        eprintln!("warning: {}: {e}", path.display());
                        CapsMap::rvs_new()
                    }),
                    Err(e) => {
                        eprintln!("warning: {}: {e}", path.display());
                        CapsMap::rvs_new()
                    }
                }
            });
            return;
        }
        // Second: caps/ directory — try CARGO_MANIFEST_DIR first, then exe dir
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let caps_dir = manifest_dir.join("caps");
        let caps_dir = if caps_dir.is_dir() {
            caps_dir
        } else {
            // Fallback: look for caps/ relative to the linter binary itself
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let exe_caps = exe_dir.join("caps");
            if exe_caps.is_dir() {
                exe_caps
            } else {
                self.capsmap = Some(CapsMap::rvs_new());
                return;
            }
        };
        self.capsmap = Some(match CapsMap::rvs_load_from_dir_BIMS(&caps_dir) {
            Ok(cm) => cm,
            Err(e) => {
                eprintln!("warning: caps/: {e}");
                CapsMap::rvs_new()
            }
        });
    }

    fn rvs_lookup_caps(&self, name: &str) -> Option<&CapabilitySet> {
        self.capsmap.as_ref()?.rvs_lookup(name)
    }

    fn rvs_is_pub_impl_item(cx: &LateContext<'_>, impl_item: &ImplItem<'_>) -> bool {
        matches!(impl_item.impl_kind, ImplItemImplKind::Inherent { .. })
            && cx.tcx.visibility(impl_item.owner_id.def_id).is_public()
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

// ─── LateLintPass ────────────────────────────────────────────────────────

impl<'tcx> LateLintPass<'tcx> for RivusLintPass {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        self.rvs_ensure_capsmap_BIMS();

        // Pre-scan: collect names of test functions by looking for consts with
        // #[rustc_test_marker], which the #[test] macro generates alongside the
        // original function. The original function loses its #[test] attribute,
        // so we detect tests via these generated consts.
        if cx.tcx.sess.is_test_crate() {
            let krate = cx.tcx.hir_crate_items(());
            for owner in krate.owners() {
                let hir_id = rustc_hir::HirId::from(owner);
                let attrs = cx.tcx.hir_attrs(hir_id);
                for a in attrs {
                    if let rustc_hir::Attribute::Parsed(AttributeKind::RustcTestMarker(_)) = a {
                        // The owner is a const generated by #[test] with the same
                        // name as the test function.
                        let node = cx.tcx.hir_node_by_def_id(owner.def_id);
                        if let rustc_hir::Node::Item(item) = node {
                            if let ItemKind::Const(ct, ..) = &item.kind {
                                self.test_fn_names.insert(ct.name.as_str().to_string());
                            }
                        }
                    }
                }
            }
        }

        if self.should_emit_lints {
            let attrs = cx.tcx.hir_attrs(rustc_hir::CRATE_HIR_ID);
            let deny_sym = Symbol::intern("deny");
            let warnings_sym = Symbol::intern("warnings");
            for a in attrs {
                if a.name() == Some(deny_sym) {
                    if let Some(items) = a.meta_item_list() {
                        for m in &items {
                            if let Some(p) = m.ident() {
                                if p.name == warnings_sym {
                                    cx.emit_span_lint(
                                        RVS_DENY_WARNINGS,
                                        a.span(),
                                        Msg::new(
                                            a.span(),
                                            "#![deny(warnings)] — use named lints instead",
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        if self.done_crate_level {
            return;
        }
        self.done_crate_level = true;

        for (name, spans) in &self.test_names {
            if spans.len() > 1 {
                for sp in spans {
                    cx.emit_span_lint(
                        RVS_DUPLICATE_TEST,
                        *sp,
                        Msg::new(*sp, format!("duplicate test '{name}'")),
                    );
                }
            }
        }

        if Path::new("test_out").is_dir() {
            for (name, spans) in &self.test_names {
                let out_file = format!("test_out/{name}.out");
                if !Path::new(&out_file).exists() {
                    if let Some(sp) = spans.first() {
                        cx.emit_span_lint(
                            RVS_MISSING_TEST_OUTPUT,
                            *sp,
                            Msg::new(*sp, format!("test '{name}' missing {out_file}")),
                        );
                    }
                }
            }
        }

        for (name, span) in &self.good_fns {
            if !self.test_call_names.contains(name)
                && !self
                    .test_call_names
                    .iter()
                    .any(|tc| tc.rsplit("::").next().unwrap_or(tc) == name.as_str())
            {
                cx.emit_span_lint(
                    RVS_UNTESTED_GOOD_FN,
                    *span,
                    Msg::new(*span, format!("good fn '{name}' not called by any test")),
                );
            }
        }

        if self.emit_report {
            if self.fn_report.is_empty() {
                return;
            }
            if let Ok(json) = serde_json::to_string(&self.fn_report) {
                let report_dir = std::env::var("RIVUS_REPORT_DIR")
                    .unwrap_or_else(|_| "target/rivus-report".into());
                let crate_name = cx
                    .tcx
                    .crate_name(rustc_span::def_id::LOCAL_CRATE)
                    .as_str()
                    .to_string();
                let report_path = Path::new(&report_dir).join(format!("{crate_name}.json"));
                if let Some(parent) = report_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut f) = std::fs::File::create(&report_path) {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                    let _ = f.sync_all();
                }
            }
        }

        if self.collect_callgraph {
            if self.callgraph.is_empty() {
                return;
            }
            if let Ok(json) = serde_json::to_string(&self.callgraph) {
                let cg_dir = std::env::var("RIVUS_CALLGRAPH_DIR")
                    .unwrap_or_else(|_| "target/rivus-callgraph".into());
                let crate_name = cx
                    .tcx
                    .crate_name(rustc_span::def_id::LOCAL_CRATE)
                    .as_str()
                    .to_string();
                let cg_path = Path::new(&cg_dir).join(format!("{crate_name}.json"));
                if let Some(parent) = cg_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut f) = std::fs::File::create(&cg_path) {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                    let _ = f.sync_all();
                }
            }
        }
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
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
                let is_test = rvs_has_attr(attrs, "test") || self.test_fn_names.contains(name);
                if self.should_emit_lints {
                    self.rvs_check_fn_MS(
                        cx,
                        name,
                        item.hir_id(),
                        item.span,
                        sig,
                        body,
                        *has_body,
                        is_test,
                        false,
                    );
                }
                self.rvs_collect_callgraph_for_item_M(cx, item.hir_id(), sig, body, false);
                if self.should_emit_lints && is_test {
                    self.test_names
                        .entry(name.to_string())
                        .or_default()
                        .push(item.span);
                    rvs_collect_test_call_names_M(cx.tcx, body, &mut self.test_call_names);
                }
                if self.should_emit_lints {
                    let vis = cx.tcx.visibility(item.owner_id.def_id);
                    let is_pub = vis.is_public();
                    rvs_check_missing_doc_S(cx, name, item.span, attrs, is_pub);
                    rvs_check_missing_safety_doc_S(
                        cx,
                        item.hir_id(),
                        item.span,
                        &sig.header.safety,
                    );
                    rvs_check_todo_comments_source_S(cx, item.span);
                }
            }
            ItemKind::Use(path, use_kind) => {
                if !self.should_emit_lints {
                    return;
                }
                for seg in path.segments {
                    let n = seg.ident.name.as_str();
                    if n == "anyhow" || n == "eyre" || n == "color_eyre" {
                        cx.emit_span_lint(
                            RVS_BANNED_IMPORT,
                            item.span,
                            Msg::new(item.span, format!("banned import: {n}")),
                        );
                    }
                }
                let ps: Vec<_> = path
                    .segments
                    .iter()
                    .map(|s| s.ident.name.as_str())
                    .collect();
                if matches!(use_kind, UseKind::Glob) {
                    if ps.last().map(|s| *s) == Some("prelude") { /* prelude ok */
                    } else if ps.len() == 1 && ps[0] == "super" { /* super ok */
                    } else {
                        let full = format!("{}::*", ps.join("::"));
                        cx.emit_span_lint(
                            RVS_WILDCARD_IMPORT,
                            item.span,
                            Msg::new(item.span, format!("wildcard import: {full}")),
                        );
                    }
                }
            }
            ItemKind::Enum(_, _, enum_def) => {
                if !self.should_emit_lints {
                    return;
                }
                let attrs = cx.tcx.hir_attrs(item.hir_id());
                let name = cx.tcx.item_name(item.owner_id.def_id);
                if !rvs_has_debug_derive(cx, item.owner_id.def_id.into()) {
                    cx.emit_span_lint(
                        RVS_MISSING_DEBUG_DERIVE,
                        item.span,
                        Msg::new(
                            item.span,
                            format!("type '{}' missing #[derive(Debug)]", name),
                        ),
                    );
                }
                let name_s = name.as_str();
                if name_s.contains("Error") || rvs_has_attr(attrs, "error") {
                    for v in enum_def.variants {
                        let vn = v.ident.name.as_str();
                        if CATCH_ALL_VARIANT_NAMES.contains(&vn) {
                            cx.emit_span_lint(
                                RVS_CATCH_ALL_ERROR_VARIANT,
                                v.span,
                                Msg::new(v.span, format!("catch-all variant '{vn}' in {name_s}")),
                            );
                        }
                    }
                }
            }
            ItemKind::Struct(_, _, data) => {
                if !self.should_emit_lints {
                    return;
                }
                let name = cx.tcx.item_name(item.owner_id.def_id);
                if !rvs_has_debug_derive(cx, item.owner_id.def_id.into()) {
                    cx.emit_span_lint(
                        RVS_MISSING_DEBUG_DERIVE,
                        item.span,
                        Msg::new(
                            item.span,
                            format!("type '{}' missing #[derive(Debug)]", name),
                        ),
                    );
                }
                if let VariantData::Struct { fields, .. } = data {
                    rvs_check_borrowed_fields_S(cx, fields);
                }
            }
            ItemKind::Impl(imp) => {
                if !self.should_emit_lints {
                    return;
                }
                if let Some(trait_ref) = &imp.of_trait {
                    if let Some(did) = trait_ref.trait_ref.trait_def_id() {
                        let trait_name = cx.tcx.item_name(did);
                        if trait_name.as_str() == "Into" {
                            cx.emit_span_lint(
                                RVS_INTO_IMPL,
                                item.span,
                                Msg::new(
                                    item.span,
                                    "impl Into — implement From instead (Into is auto-provided)",
                                ),
                            );
                        }
                        if trait_name.as_str() == "Deref" {
                            cx.emit_span_lint(
                                RVS_DEREF_POLYMORPHISM,
                                item.span,
                                Msg::new(
                                    item.span,
                                    "impl Deref — use composition instead of Deref polymorphism",
                                ),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, impl_item: &'tcx ImplItem<'tcx>) {
        if let ImplItemKind::Fn(sig, body_id) = &impl_item.kind {
            let parent = cx.tcx.hir_get_parent_item(impl_item.hir_id());
            let parent_node = cx.tcx.hir_owner_node(parent);
            let is_trait_impl = matches!(
                parent_node,
                rustc_hir::OwnerNode::Item(Item {
                    kind: ItemKind::Impl(Impl {
                        of_trait: Some(_),
                        ..
                    }),
                    ..
                })
            );
            let name = impl_item.ident.name.as_str();
            let body = cx.tcx.hir_body(*body_id);
            let attrs = cx.tcx.hir_attrs(impl_item.hir_id());
            let is_test = rvs_has_attr(attrs, "test");
            let is_pub = Self::rvs_is_pub_impl_item(cx, impl_item);
            if self.should_emit_lints && !is_trait_impl {
                self.rvs_check_fn_MS(
                    cx,
                    name,
                    impl_item.hir_id(),
                    impl_item.span,
                    sig,
                    body,
                    true,
                    is_test,
                    false,
                );
            }
            self.rvs_collect_callgraph_for_item_M(cx, impl_item.hir_id(), sig, body, is_trait_impl);
            if self.should_emit_lints && is_test {
                self.test_names
                    .entry(name.to_string())
                    .or_default()
                    .push(impl_item.span);
                rvs_collect_test_call_names_M(cx.tcx, body, &mut self.test_call_names);
            }
            if self.should_emit_lints && !is_test && is_pub && !is_trait_impl {
                rvs_check_missing_doc_S(cx, name, impl_item.span, attrs, true);
            }
            if self.should_emit_lints && is_pub && !is_trait_impl {
                rvs_check_missing_safety_doc_S(
                    cx,
                    impl_item.hir_id(),
                    impl_item.span,
                    &sig.header.safety,
                );
            }
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, trait_item: &'tcx TraitItem<'tcx>) {
        if let TraitItemKind::Fn(sig, TraitFn::Provided(body_id)) = &trait_item.kind {
            let name = trait_item.ident.name.as_str();
            let body = cx.tcx.hir_body(*body_id);
            if self.should_emit_lints {
                self.rvs_check_fn_MS(
                    cx,
                    name,
                    trait_item.hir_id(),
                    trait_item.span,
                    sig,
                    body,
                    true,
                    false,
                    true,
                );
            }
            self.rvs_collect_callgraph_for_item_M(cx, trait_item.hir_id(), sig, body, false);
        }
    }
}

// ─── Attribute helpers ───────────────────────────────────────────────────

fn rvs_has_attr(attrs: &[rustc_hir::Attribute], name: &str) -> bool {
    let sym = Symbol::intern(name);
    attrs.iter().any(|a| {
        if a.has_name(sym) {
            return true;
        }
        // The #[test] macro transforms #[test] into a parsed
        // Attribute::Parsed(AttributeKind::RustcTestMarker(...)).
        // has_name only works for Unparsed attributes, so we check the
        // parsed form explicitly.
        if name == "test" {
            if let rustc_hir::Attribute::Parsed(AttributeKind::RustcTestMarker(_)) = a {
                return true;
            }
        }
        false
    })
}

fn rvs_has_allow(attrs: &[rustc_hir::Attribute], lint_name: &str) -> bool {
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

fn rvs_allows_non_snake_case(cx: &LateContext<'_>, hir_id: HirId) -> bool {
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

fn rvs_has_doc_section(cx: &LateContext<'_>, hir_id: HirId, section: &str) -> bool {
    for a in cx.tcx.hir_attrs(hir_id) {
        if let Some(d) = a.doc_str() {
            if d.as_str().trim().starts_with(&format!("# {section}")) {
                return true;
            }
        }
    }
    false
}

fn rvs_has_any_doc(attrs: &[rustc_hir::Attribute]) -> bool {
    for a in attrs {
        if a.doc_str().is_some() {
            return true;
        }
    }
    false
}

fn rvs_has_debug_derive(cx: &LateContext<'_>, def_id: rustc_span::def_id::DefId) -> bool {
    // In nightly Rust, derive attributes are consumed during macro expansion
    // and not retained in the HIR attrs. Instead, check if the type implements
    // Debug by looking at the trait_impls_of query.
    let debug_did = match cx.tcx.get_diagnostic_item(Symbol::intern("Debug")) {
        Some(did) => did,
        None => return true, // can't find Debug trait, assume it's implemented
    };
    let impls = cx.tcx.trait_impls_of(debug_did);
    let item_ty = cx.tcx.type_of(def_id).skip_binder();
    // Check non-blanket impls and blanket impls
    impls.non_blanket_impls().values().any(|impls_dids| {
        impls_dids
            .iter()
            .any(|impl_did| cx.tcx.type_of(*impl_did).skip_binder() == item_ty)
    }) || impls
        .blanket_impls()
        .iter()
        .any(|impl_did| cx.tcx.type_of(*impl_did).skip_binder() == item_ty)
}

// ─── FnInfo ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct FnInfo {
    caps: CapabilitySet,
    raw_suffix: String,
    is_async: bool,
    is_unsafe_fn: bool,
    has_mut_param: bool,
    has_unsafe_block: bool,
}

impl FnInfo {
    fn rvs_extract<'tcx>(
        name: &str,
        sig: &rustc_hir::FnSig<'_>,
        body: &Body<'tcx>,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> Option<Self> {
        let (_, caps) = rvs_parse_function(name)?;
        Some(Self {
            caps,
            raw_suffix: rvs_extract_raw_suffix(name),
            is_async: sig.header.asyncness.is_async(),
            is_unsafe_fn: matches!(
                sig.header.safety,
                rustc_hir::HeaderSafety::Normal(Safety::Unsafe)
            ),
            has_mut_param: rvs_has_mutable_params(sig, body),
            has_unsafe_block: rvs_scan_unsafe(tcx, body),
        })
    }
}

/// Check if a function has mutable parameters — either `&mut` references
/// in the type signature. This drives the M capability from the signature.
///
/// Note: `mut foo: T` bindings on by-value parameters are NOT counted as M
/// because in Rust 2024 edition, the `mut` keyword on parameters is purely
/// cosmetic (the HIR records `BindingMode(No, Not)` for all params). Moreover,
/// a by-value `mut` binding only modifies a local copy — it doesn't affect the
/// caller's state. Only `&mut` references truly modify the caller's state.
fn rvs_has_mutable_params(sig: &rustc_hir::FnSig<'_>, _body: &Body<'_>) -> bool {
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

fn rvs_scan_unsafe<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &Body<'tcx>) -> bool {
    let mut f = false;
    rvs_walk_closures(tcx, body.value, |e| {
        if f {
            return;
        }
        if let ExprKind::Block(b, _) = &e.kind {
            if matches!(b.rules, BlockCheckMode::UnsafeBlock(_)) {
                // format_args! macro expansion introduces `unsafe { Arguments::new(...) }`
                // as an implementation detail. Skip these — the user didn't write the unsafe block.
                if !b.span.from_expansion() {
                    f = true;
                }
            }
        }
    });
    f
}

fn rvs_scan_stub<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &Body<'tcx>) -> bool {
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
        // todo!() and unimplemented!() expand to core::panicking::panic(...)
        // in HIR, so the path-based check above won't match. Detect them by
        // checking whether the call originated from a todo!/unimplemented!
        // macro expansion.
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
            // Also check the outermost expansion itself
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

/// Returns (is_empty, only_debug_asserts).
/// is_empty: the body has no effective logic.
/// only_debug_asserts: true if the body contained debug_assert! calls (even if empty).
fn rvs_is_empty_body(body: &Body<'_>) -> (bool, bool) {
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

fn rvs_scan_debug_asserts_M<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> BTreeSet<String> {
    let da = Symbol::intern("debug_assert");
    let dae = Symbol::intern("debug_assert_eq");
    let dan = Symbol::intern("debug_assert_ne");
    let mut out = BTreeSet::new();
    rvs_walk_closures(tcx, body.value, |e| {
        // debug_assert! macros are expanded in the HIR. We detect them by
        // checking if an expression originates from a debug_assert expansion.
        // Then we deeply collect all idents from within the expansion.
        if e.span.from_expansion() {
            let mut expn_id = e.span.ctxt().outer_expn_data().parent;
            let mut is_debug_assert = false;
            // Check the outermost expansion itself
            let outer_expn = e.span.ctxt().outer_expn_data();
            if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = outer_expn.kind
            {
                if name == da || name == dae || name == dan {
                    is_debug_assert = true;
                }
            }
            // Walk ancestor expansions
            while expn_id != rustc_span::ExpnId::root() {
                let expn = expn_id.expn_data();
                if let rustc_span::ExpnKind::Macro(rustc_span::MacroKind::Bang, name) = expn.kind {
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
    });
    out
}

/// Deeply collect all path idents from an expression tree, recursing into
/// blocks, calls, matches, and all other expression kinds.
/// Used for collecting param names from inside debug_assert! expanded code,
/// where macro hygiene may cause sub-expressions to lose their expansion context.
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

fn rvs_scan_static_refs_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Body<'tcx>,
) -> Vec<(Span, CapabilitySet, bool)> {
    let mut refs = Vec::new();
    rvs_walk_closures(cx.tcx, body.value, |e| {
        if let ExprKind::Path(ref q) = e.kind {
            if let Res::Def(kind, did) = cx.qpath_res(q, e.hir_id) {
                match kind {
                    DefKind::Static {
                        mutability: Mutability::Mut,
                        ..
                    } => {
                        let required = {
                            let mut cs = CapabilitySet::rvs_new();
                            cs.rvs_insert_M(Capability::S);
                            cs.rvs_insert_M(Capability::U);
                            cs
                        };
                        refs.push((e.span, required, false));
                    }
                    DefKind::Static {
                        mutability: Mutability::Not,
                        ..
                    } => {
                        let mut cs = CapabilitySet::rvs_new();
                        cs.rvs_insert_M(Capability::S);
                        let mut is_thread_local = false;
                        if let Some(local_did) = did.as_local() {
                            let owner_id = rustc_hir::OwnerId { def_id: local_did };
                            let attrs = cx.tcx.hir_attrs(rustc_hir::HirId::from(owner_id));
                            is_thread_local = rvs_has_attr(attrs, "thread_local");
                        }
                        if is_thread_local {
                            cs.rvs_insert_M(Capability::T);
                        }
                        refs.push((e.span, cs, is_thread_local));
                    }
                    _ => {}
                }
            }
        }
    });
    refs
}

fn rvs_collect_test_call_names_M<'tcx>(
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

fn rvs_count_effective_lines_M<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>) -> usize {
    let source_map = cx.tcx.sess.source_map();
    let block = match &body.value.kind {
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
        if in_block_comment {
            // We're inside a block comment from a previous line.
            // Use the same scanner to find the closing */ and check for effective code.
            if rvs_line_has_effective_code_M(trimmed, &mut in_block_comment) {
                count += 1;
            }
            continue;
        }
        if rvs_line_has_effective_code_M(trimmed, &mut in_block_comment) {
            count += 1;
        }
    }
    count
}

/// Scan a line to determine if it contains effective (non-comment, non-whitespace) code.
/// Handles `/* ... */` block comments that open and close on the same line.
/// Sets `in_comment` to true if a block comment is opened but not closed on this line.
fn rvs_line_has_effective_code_M(line: &str, in_comment: &mut bool) -> bool {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut has_code = false;
    while i < len {
        if *in_comment {
            if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                *in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            *in_comment = true;
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            // String literal — count as code
            has_code = true;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment — everything after is comment
            break;
        }
        if !matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
            has_code = true;
        }
        i += 1;
    }
    has_code
}

// ─── Walker ──────────────────────────────────────────────────────────────

fn rvs_walk_closures<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    e: &'tcx Expr<'tcx>,
    mut f: F,
) {
    fn go<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
        e: &'tcx Expr<'tcx>,
        f: &mut F,
        resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
        depth: u32,
    ) {
        if depth > 16 {
            return;
        }
        f(e);
        match &e.kind {
            ExprKind::Array(a) | ExprKind::Tup(a) => {
                a.iter().for_each(|x| go(x, f, resolve_body, depth))
            }
            ExprKind::Call(fn_, a) => {
                go(fn_, f, resolve_body, depth);
                a.iter().for_each(|x| go(x, f, resolve_body, depth));
            }
            ExprKind::MethodCall(_, r, a, _) => {
                go(r, f, resolve_body, depth);
                a.iter().for_each(|x| go(x, f, resolve_body, depth));
            }
            ExprKind::Binary(_, l, r) | ExprKind::AssignOp(_, l, r) => {
                go(l, f, resolve_body, depth);
                go(r, f, resolve_body, depth);
            }
            ExprKind::Unary(_, x)
            | ExprKind::Cast(x, _)
            | ExprKind::Type(x, _)
            | ExprKind::Field(x, _)
            | ExprKind::Index(x, _, _)
            | ExprKind::AddrOf(_, _, x)
            | ExprKind::Repeat(x, _)
            | ExprKind::Yield(x, _) => go(x, f, resolve_body, depth),
            ExprKind::Let(l) => go(&l.init, f, resolve_body, depth),
            ExprKind::If(c, t, el) => {
                go(c, f, resolve_body, depth);
                go(t, f, resolve_body, depth);
                if let Some(e) = el {
                    go(e, f, resolve_body, depth);
                }
            }
            ExprKind::Match(s, arms, _) => {
                go(s, f, resolve_body, depth);
                for arm in *arms {
                    if let Some(guard) = arm.guard {
                        go(guard, f, resolve_body, depth);
                    }
                    go(&arm.body, f, resolve_body, depth);
                }
            }
            ExprKind::Loop(b, ..) | ExprKind::Block(b, _) => wblk(b, f, resolve_body, depth),
            ExprKind::Assign(l, r, _) => {
                go(l, f, resolve_body, depth);
                go(r, f, resolve_body, depth);
            }
            ExprKind::Break(_, Some(x)) | ExprKind::Ret(Some(x)) => {
                go(&**x, f, resolve_body, depth)
            }
            ExprKind::Struct(_, fld, rest) => {
                fld.iter()
                    .for_each(|fl| go(&fl.expr, f, resolve_body, depth));
                if let rustc_hir::StructTailExpr::Base(r) = rest {
                    go(r, f, resolve_body, depth);
                }
            }
            ExprKind::Closure(closure) => {
                if let Some(body) = resolve_body(closure.body) {
                    go(body.value, f, resolve_body, depth + 1);
                }
            }
            ExprKind::InlineAsm(asm) => asm.operands.iter().for_each(|(op, _)| match op {
                rustc_hir::InlineAsmOperand::In { expr, .. }
                | rustc_hir::InlineAsmOperand::Out {
                    expr: Some(expr), ..
                } => go(expr, f, resolve_body, depth),
                _ => {}
            }),
            ExprKind::DropTemps(x) => go(x, f, resolve_body, depth),
            ExprKind::Become(x) => go(x, f, resolve_body, depth),
            ExprKind::Use(x, _) => go(x, f, resolve_body, depth),
            _ => {}
        }
    }
    fn wblk<'tcx, F: FnMut(&'tcx Expr<'tcx>)>(
        b: &'tcx Block<'tcx>,
        f: &mut F,
        resolve_body: &dyn Fn(rustc_hir::BodyId) -> Option<&'tcx Body<'tcx>>,
        depth: u32,
    ) {
        for s in b.stmts {
            match &s.kind {
                rustc_hir::StmtKind::Expr(e) | rustc_hir::StmtKind::Semi(e) => {
                    go(e, f, resolve_body, depth)
                }
                rustc_hir::StmtKind::Let(l) => {
                    if let Some(i) = l.init {
                        go(i, f, resolve_body, depth);
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = b.expr {
            go(e, f, resolve_body, depth);
        }
    }
    let resolver = |bid: rustc_hir::BodyId| -> Option<&'tcx Body<'tcx>> { Some(tcx.hir_body(bid)) };
    go(e, &mut f, &resolver, 0);
}

// ─── Path helpers ────────────────────────────────────────────────────────

fn rvs_qp(q: &QPath<'_>) -> String {
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

fn rvs_tys(t: &rustc_hir::Ty<'_>) -> String {
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

fn rvs_plast(q: &QPath<'_>) -> Option<String> {
    match q {
        QPath::Resolved(_, p) => p.segments.last().map(|s| s.ident.name.to_string()),
        QPath::TypeRelative(_, s) => Some(s.ident.name.to_string()),
    }
}

fn rvs_def_path(cx: &LateContext<'_>, did: DefId) -> String {
    let tcx = cx.tcx;
    let dp = tcx.def_path(did);
    // If `did` is an associated item (method) inside an inherent impl,
    // we recover the self-type name from the impl block to insert into
    // the path.  `DefPathData::Impl` does not carry the type name — we
    // must look it up via `tcx.type_of(impl_def_id)`.
    let inherent_impl_ty: Option<String> = cx
        .tcx
        .opt_associated_item(did)
        .map(|assoc| (assoc, assoc.container_id(cx.tcx)))
        .and_then(|(_, impl_def_id)| {
            if let rustc_hir::def::DefKind::Impl { of_trait: false } = cx.tcx.def_kind(impl_def_id)
            {
                rvs_inherent_impl_type_name(cx, impl_def_id)
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
                // For inherent impls, insert the self-type name so that
                // methods on different types with the same name don't
                // collide (e.g. `SystemTime::now` vs `Instant::now`).
                // For trait impls, the `@TraitPath` suffix handles
                // disambiguation.
                if let Some(ref ty_name) = inherent_impl_ty {
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

/// Extract the type name from an inherent impl block's self-type.
///
/// For `impl SystemTime { fn now() }` this returns `"SystemTime"`.
/// Returns `None` for complex types (generics, tuples, etc.) where a
/// simple name cannot be extracted.
fn rvs_inherent_impl_type_name(cx: &LateContext<'_>, impl_def_id: DefId) -> Option<String> {
    let self_ty = cx.tcx.type_of(impl_def_id).skip_binder();
    let ty_str = self_ty.to_string();
    // Extract the last path segment — for `SystemTime` this is just
    // "SystemTime", for `alloc::vec::Vec<T>` it would be "Vec".
    // We only care about the simple type name, not generic args.
    match self_ty.kind() {
        rustc_middle::ty::TyKind::Adt(adt_def, _) => {
            cx.tcx.item_name(adt_def.did()).to_string().into()
        }
        _ => {
            // Fallback: take the last `::` segment of the type string
            ty_str.rsplit("::").next().map(|s| s.to_string())
        }
    }
}

fn rvs_ty_last_ident(ty: &rustc_hir::Ty<'_>) -> Option<String> {
    match &ty.kind {
        TyKind::Path(q) => rvs_plast(q),
        TyKind::Ref(_, mt) => rvs_ty_last_ident(mt.ty),
        _ => None,
    }
}

fn rvs_generic_args_result_type<'a>(
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

// ─── Core check ──────────────────────────────────────────────────────────

impl RivusLintPass {
    fn rvs_check_fn_MS<'tcx>(
        &mut self,
        cx: &LateContext<'tcx>,
        name: &str,
        hir_id: HirId,
        span: Span,
        sig: &rustc_hir::FnSig<'tcx>,
        body: &Body<'tcx>,
        has_body: bool,
        is_test: bool,
        is_trait_impl_method: bool,
    ) {
        let attrs = cx.tcx.hir_attrs(hir_id);

        if let Some(info) = FnInfo::rvs_extract(name, sig, body, cx.tcx) {
            let is_stub = rvs_scan_stub(cx.tcx, body);
            if is_stub {
                cx.emit_span_lint(
                    RVS_STUB_MACRO,
                    span,
                    Msg::new(span, "stub: todo!()/unimplemented!()"),
                );
            }
            if has_body && !is_stub {
                let (is_empty, only_debug_asserts) = rvs_is_empty_body(body);
                if is_empty {
                    let msg = if only_debug_asserts {
                        "function body contains only debug_assert!"
                    } else {
                        "empty function body"
                    };
                    cx.emit_span_lint(RVS_EMPTY_FN, span, Msg::new(span, msg));
                }
            }
            if !info.raw_suffix.is_empty() && !rvs_allows_non_snake_case(cx, hir_id) {
                cx.emit_span_lint(
                    RVS_MISSING_ALLOW,
                    span,
                    Msg::new(span, "uppercase suffix without #[allow(non_snake_case)]"),
                );
            }
            if rvs_has_allow(attrs, "dead_code") || rvs_has_allow(attrs, "unused") {
                cx.emit_span_lint(
                    RVS_DEAD_CODE,
                    span,
                    Msg::new(span, "rvs_ function marked #[allow(dead_code/unused)]"),
                );
            }
            if info.is_async && !info.caps.rvs_contains(Capability::A) {
                cx.emit_span_lint(
                    RVS_MISSING_ASYNC,
                    span,
                    Msg::new(span, "async but suffix lacks A"),
                );
            }
            if info.is_unsafe_fn && !info.caps.rvs_contains(Capability::U) {
                cx.emit_span_lint(
                    RVS_MISSING_UNSAFE,
                    span,
                    Msg::new(span, "unsafe code but suffix lacks U"),
                );
            }
            if info.has_mut_param && !info.caps.rvs_contains(Capability::M) {
                cx.emit_span_lint(
                    RVS_MISSING_MUTABLE,
                    span,
                    Msg::new(span, "&mut param but suffix lacks M"),
                );
            }
            let raw = &info.raw_suffix;
            if !raw.is_empty() {
                let mut cv: Vec<char> = raw.chars().collect();
                cv.sort();
                let sorted: String = cv.into_iter().collect();
                if raw != &sorted {
                    cx.emit_span_lint(
                        RVS_NON_ALPHABETICAL_SUFFIX,
                        span,
                        Msg::new(span, "suffix not alphabetical"),
                    );
                }
                let mut seen = HashSet::new();
                for c in raw.chars() {
                    if !seen.insert(c) {
                        cx.emit_span_lint(
                            RVS_DUPLICATE_SUFFIX,
                            span,
                            Msg::new(span, format!("duplicate '{c}'")),
                        );
                        break;
                    }
                }
                let unk = rvs_extract_unknown_suffix_letters(raw);
                if !unk.is_empty() {
                    cx.emit_span_lint(
                        RVS_UNKNOWN_SUFFIX_LETTER,
                        span,
                        Msg::new(
                            span,
                            format!(
                                "unknown letters: {}",
                                unk.iter()
                                    .map(|c| c.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ),
                    );
                }
            }

            self.rvs_check_calls_MS(cx, body, hir_id, &info.caps, is_test, name, &info);
            if has_body && !is_stub {
                self.rvs_check_static_refs_MS(cx, body, &info.caps);
                self.rvs_check_debug_asserts_MS(cx, body);
                self.rvs_check_borrowed_params_S(cx, sig);
                self.rvs_check_consumed_arg_on_error_MS(cx, sig, name);
                self.rvs_check_validate_returns_unit_S(cx, name, sig);
            }

            let good = CapabilitySet::rvs_from_good_caps();
            if info.caps.rvs_is_subset_of(&good)
                && !is_test
                && !rvs_has_allow(attrs, "dead_code")
                && !rvs_has_allow(attrs, "unused")
            {
                self.good_fns.push((name.to_string(), span));
            }

            let allows_dead_code =
                rvs_has_allow(attrs, "dead_code") || rvs_has_allow(attrs, "unused");
            let effective_lines = if has_body {
                rvs_count_effective_lines_M(cx, body)
            } else {
                0
            };
            let caps_str: String = info.caps.rvs_iter().map(|c| c.rvs_as_char()).collect();
            self.fn_report.push(FnReportEntry {
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
            if name.starts_with(|c: char| c.is_ascii_lowercase()) {
                cx.emit_span_lint(
                    RVS_NON_RVS_FN,
                    span,
                    Msg::new(span, format!("'{name}' missing rvs_ prefix")),
                );
            }
        }
        if is_test && !rvs_valid_test(name) {
            cx.emit_span_lint(
                RVS_TEST_NAME_FORMAT,
                span,
                Msg::new(span, format!("test '{name}' not test_YYYYMMDD_name")),
            );
        }
    }

    fn rvs_check_calls_MS<'tcx>(
        &mut self,
        cx: &LateContext<'tcx>,
        body: &Body<'tcx>,
        _hir_id: HirId,
        caps: &CapabilitySet,
        is_test: bool,
        caller_name: &str,
        info: &FnInfo,
    ) {
        let collect_cg = self.collect_callgraph;
        let mut cg_edges: Vec<String> = Vec::new();
        let mut has_static_ref = false;
        let mut has_static_mut_ref = false;
        let mut has_thread_local_ref = false;
        if collect_cg {
            rvs_walk_closures(cx.tcx, body.value, |e| {
                if let ExprKind::Path(ref q) = e.kind {
                    if let Res::Def(kind, did) = cx.qpath_res(q, e.hir_id) {
                        if let DefKind::Static { mutability, .. } = kind {
                            match mutability {
                                Mutability::Mut => has_static_mut_ref = true,
                                Mutability::Not => {
                                    if let Some(local_did) = did.as_local() {
                                        let owner_id = rustc_hir::OwnerId { def_id: local_did };
                                        let attrs =
                                            cx.tcx.hir_attrs(rustc_hir::HirId::from(owner_id));
                                        if rvs_has_attr(attrs, "thread_local") {
                                            has_thread_local_ref = true;
                                        }
                                    }
                                    has_static_ref = true;
                                }
                            }
                        }
                    }
                }
            });
        }
        rvs_walk_closures(cx.tcx, body.value, |e| match &e.kind {
            ExprKind::Call(func, _) => {
                if let ExprKind::Path(ref q) = func.kind {
                    if let Res::Def(k, did) = cx.qpath_res(q, func.hir_id) {
                        if matches!(k, DefKind::Fn | DefKind::AssocFn | DefKind::Variant) {
                            let fp = rvs_def_path(cx, did);
                            if collect_cg {
                                cg_edges.push(fp.clone());
                            }
                            if !is_test && rvs_is_spawn_S(&fp) {
                                cx.emit_span_lint(
                                    RVS_SPAWN_WARNING,
                                    e.span,
                                    Msg::new(
                                        e.span,
                                        format!("spawn: {fp} — use structured concurrency"),
                                    ),
                                );
                            }
                            if rvs_is_reflection_S(&fp) {
                                cx.emit_span_lint(
                                    RVS_REFLECTION_USAGE,
                                    e.span,
                                    Msg::new(e.span, "reflection — use trait dispatch instead"),
                                );
                            }
                            let sp = rvs_qp(q);
                            self.rvs_check_target_S(cx, e.span, &fp, Some(&sp), caps);
                            let cn = fp.rsplit("::").next().unwrap_or(&fp);
                            if cn == "catch_unwind" {
                                cx.emit_span_lint(
                                    RVS_CATCH_UNWIND,
                                    e.span,
                                    Msg::new(e.span, "catch_unwind — fix panic source instead"),
                                );
                            }
                        }
                    } else {
                        let ps = rvs_qp(q);
                        if !is_test && rvs_is_spawn_S(&ps) {
                            cx.emit_span_lint(
                                RVS_SPAWN_WARNING,
                                e.span,
                                Msg::new(
                                    e.span,
                                    format!("spawn: {ps} — use structured concurrency"),
                                ),
                            );
                        }
                        if rvs_is_reflection_S(&ps) {
                            cx.emit_span_lint(
                                RVS_REFLECTION_USAGE,
                                e.span,
                                Msg::new(e.span, "reflection — use trait dispatch instead"),
                            );
                        }
                    }
                }
            }
            ExprKind::MethodCall(p, ..) => {
                let n = p.ident.name.as_str();
                if ERROR_SWALLOW_METHODS.contains(&n) {
                    cx.emit_span_lint(
                        RVS_ERROR_SWALLOW,
                        e.span,
                        Msg::new(e.span, format!(".{n}() swallows errors")),
                    );
                }
                if n == "catch_unwind" {
                    cx.emit_span_lint(
                        RVS_CATCH_UNWIND,
                        e.span,
                        Msg::new(e.span, "catch_unwind — fix panic source instead"),
                    );
                }
                let owner = e.hir_id.owner.def_id;
                let tck = cx.tcx.typeck(owner);
                if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                    let fp = rvs_def_path(cx, did);
                    if collect_cg {
                        cg_edges.push(fp.clone());
                    }
                    if !is_test && rvs_is_spawn_S(&fp) {
                        cx.emit_span_lint(
                            RVS_SPAWN_WARNING,
                            e.span,
                            Msg::new(e.span, format!("spawn: {fp}")),
                        );
                    }
                    if rvs_is_reflection_S(&fp) {
                        cx.emit_span_lint(
                            RVS_REFLECTION_USAGE,
                            e.span,
                            Msg::new(e.span, "reflection — use trait dispatch instead"),
                        );
                    }
                    self.rvs_check_target_S(cx, e.span, &fp, Some(n), caps);
                }
            }
            _ => {}
        });
        if collect_cg {
            let entry = self
                .callgraph
                .entry(caller_name.to_string())
                .or_insert_with(|| FnBehavior {
                    calls: BTreeSet::new(),
                    has_async: info.is_async,
                    has_unsafe_block: info.has_unsafe_block,
                    is_unsafe_fn: info.is_unsafe_fn,
                    has_mut_param: info.has_mut_param,
                    has_static_ref,
                    has_static_mut_ref,
                    has_thread_local_ref,
                    is_trait_impl: false,
                });
            for callee in cg_edges {
                entry.calls.insert(callee);
            }
        }
    }

    fn rvs_check_target_S<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        span: Span,
        def_path: &str,
        src_path: Option<&str>,
        caps: &CapabilitySet,
    ) {
        let cn = def_path.rsplit("::").next().unwrap_or(def_path);
        if let Some((_, cc)) = rvs_parse_function(cn) {
            if !caps.rvs_can_call(&cc) {
                let m: Vec<_> = caps
                    .rvs_missing_for(&cc)
                    .iter()
                    .map(|c| format!("{c}"))
                    .collect();
                cx.emit_span_lint(
                    RVS_CALL_VIOLATION,
                    span,
                    Msg::new(span, format!("{} → {} missing {}", caps, cc, m.join(", "))),
                );
            }
            return;
        }
        if let Some(cc) = self
            .rvs_lookup_caps(def_path)
            .or_else(|| src_path.and_then(|s| self.rvs_lookup_caps(s)))
            .cloned()
        {
            if !cc.rvs_is_empty() && !caps.rvs_can_call(&cc) {
                let m: Vec<_> = caps
                    .rvs_missing_for(&cc)
                    .iter()
                    .map(|c| format!("{c}"))
                    .collect();
                let callee_display = if let Some(sp) = src_path {
                    if sp != def_path {
                        format!("{sp} ({def_path})")
                    } else {
                        def_path.to_string()
                    }
                } else {
                    def_path.to_string()
                };
                cx.emit_span_lint(
                    RVS_CALL_VIOLATION,
                    span,
                    Msg::new(
                        span,
                        format!(
                            "{} → {callee_display} ({}) missing {}",
                            caps,
                            cc,
                            m.join(", ")
                        ),
                    ),
                );
            }
            return;
        }
        let hint = if let Some(sp) = src_path {
            if sp != def_path {
                format!("'{sp}' ({def_path}) not in capsmap")
            } else {
                format!("'{def_path}' not in capsmap")
            }
        } else {
            format!("'{def_path}' not in capsmap")
        };
        cx.emit_span_lint(RVS_UNKNOWN_CALLEE, span, Msg::new(span, hint));
    }

    fn rvs_check_static_refs_MS<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        body: &Body<'tcx>,
        caps: &CapabilitySet,
    ) {
        let refs = rvs_scan_static_refs_M(cx, body);
        for (span, required, is_thread_local) in refs {
            if !caps.rvs_can_call(&required) {
                let missing: Vec<_> = caps
                    .rvs_missing_for(&required)
                    .iter()
                    .map(|c| format!("{c}"))
                    .collect();
                cx.emit_span_lint(
                    RVS_STATIC_REF,
                    span,
                    Msg::new(
                        span,
                        format!(
                            "static ref requires {} but fn has {} (missing {})",
                            required,
                            caps,
                            missing.join(", ")
                        ),
                    ),
                );
            }
            if required.rvs_contains(Capability::S) && !caps.rvs_contains(Capability::S) {
                cx.emit_span_lint(
                    RVS_MISSING_SIDE_EFFECT,
                    span,
                    Msg::new(span, "reads static but suffix lacks S"),
                );
            }
            if is_thread_local && !caps.rvs_contains(Capability::T) {
                cx.emit_span_lint(
                    RVS_MISSING_THREAD_LOCAL,
                    span,
                    Msg::new(span, "reads thread_local! but suffix lacks T"),
                );
            }
        }
    }

    fn rvs_check_debug_asserts_MS<'tcx>(&self, cx: &LateContext<'tcx>, body: &Body<'tcx>) {
        let owner = body.value.hir_id.owner;
        let tck = cx.tcx.typeck(owner.def_id);
        let mut prims = Vec::new();
        for p in body.params {
            let ty = tck.pat_ty(p.pat);
            let ts = ty.to_string();
            if matches!(
                ts.as_str(),
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f32"
                    | "f64"
            ) {
                if let PatKind::Binding(_, _, id, _) = p.pat.kind {
                    prims.push(id.name.to_string());
                }
            }
        }
        if prims.is_empty() {
            return;
        }
        let asserted = rvs_scan_debug_asserts_M(cx.tcx, body);
        for p in &prims {
            if !asserted.contains(p) {
                cx.emit_span_lint(
                    RVS_MISSING_DEBUG_ASSERT,
                    body.value.span,
                    Msg::new(
                        body.value.span,
                        format!("param '{p}' missing debug_assert!"),
                    ),
                );
            }
        }
    }

    fn rvs_check_borrowed_params_S<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        sig: &rustc_hir::FnSig<'tcx>,
    ) {
        for input in sig.decl.inputs {
            if let TyKind::Ref(_, mt) = &input.kind {
                if mt.mutbl == Mutability::Not {
                    if let Some(name) = rvs_ty_last_ident(mt.ty) {
                        if BORROWED_TYPES.contains(&name.as_str()) {
                            let better = match name.as_str() {
                                "String" => "&str",
                                "Vec" => "&[T]",
                                "Box" => "&T",
                                _ => continue,
                            };
                            cx.emit_span_lint(
                                RVS_BORROWED_PARAM,
                                input.span,
                                Msg::new(input.span, format!("&{name} — use {better} instead")),
                            );
                        }
                    }
                }
            }
        }
    }

    fn rvs_check_consumed_arg_on_error_MS<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        sig: &rustc_hir::FnSig<'tcx>,
        fn_name: &str,
    ) {
        let FnRetTy::Return(ret_ty) = sig.decl.output else {
            return;
        };
        let TyKind::Path(ref q) = ret_ty.kind else {
            return;
        };
        let result_name = rvs_plast(q);
        if result_name.as_deref() != Some("Result") {
            return;
        }

        let type_args = match q {
            QPath::Resolved(_, p) => p
                .segments
                .first()
                .and_then(|s| s.args)
                .map(|ga| rvs_generic_args_result_type(Some(ga)))
                .unwrap_or_else(Vec::new),
            _ => return,
        };

        if type_args.len() >= 1 {
            let ok_str = rvs_tys(type_args[0]);
            if ok_str != "()" {
                return;
            }
        }

        let mut error_idents = HashSet::new();
        if type_args.len() >= 2 {
            rvs_collect_type_idents_M(type_args[1], &mut error_idents);
        }

        for input in sig.decl.inputs {
            if let TyKind::Path(ref iq) = input.kind {
                if let Some(param_name) = rvs_plast(iq) {
                    let is_ref = matches!(input.kind, TyKind::Ref(_, _));
                    if !is_ref && !error_idents.contains(&param_name) {
                        cx.emit_span_lint(RVS_CONSUMED_ARG_ON_ERROR, input.span, Msg::new(input.span, format!("owned param '{param_name}' consumed but not preserved in error type of {fn_name}")));
                    }
                }
            }
        }
    }

    fn rvs_check_validate_returns_unit_S<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        name: &str,
        sig: &rustc_hir::FnSig<'tcx>,
    ) {
        let base = name
            .strip_prefix("rvs_")
            .unwrap_or(name)
            .split('_')
            .next()
            .unwrap_or("");
        let lower = base.to_ascii_lowercase();
        if !VALIDATE_PREFIXES.iter().any(|p| lower == *p) {
            return;
        }

        let FnRetTy::Return(ret_ty) = sig.decl.output else {
            return;
        };
        let TyKind::Path(ref q) = ret_ty.kind else {
            return;
        };
        let result_name = rvs_plast(q);
        if result_name.as_deref() != Some("Result") {
            return;
        }

        let type_args = match q {
            QPath::Resolved(_, p) => p
                .segments
                .first()
                .and_then(|s| s.args)
                .map(|ga| rvs_generic_args_result_type(Some(ga)))
                .unwrap_or_else(Vec::new),
            _ => return,
        };

        if type_args.len() >= 1 {
            let ok_str = rvs_tys(type_args[0]);
            if ok_str == "()" {
                cx.emit_span_lint(RVS_VALIDATE_RETURNS_UNIT, sig.span, Msg::new(sig.span, format!("{name}: validate returning Result<(),E> — use TryFrom returning Result<T,E>")));
            }
        }
    }

    fn rvs_collect_callgraph_for_item_M<'tcx>(
        &mut self,
        cx: &LateContext<'tcx>,
        hir_id: HirId,
        sig: &rustc_hir::FnSig<'tcx>,
        body: &Body<'tcx>,
        is_trait_impl: bool,
    ) {
        if !self.collect_callgraph {
            return;
        }
        let local_def_id = hir_id.owner.def_id;
        let def_id = local_def_id.to_def_id();
        let caller_path = rvs_def_path(cx, def_id);

        let mut calls: BTreeSet<String> = BTreeSet::new();
        let mut has_static_ref = false;
        let mut has_static_mut_ref = false;
        let mut has_thread_local_ref = false;

        rvs_walk_closures(cx.tcx, body.value, |e| {
            if let ExprKind::Path(ref q) = e.kind {
                if let Res::Def(kind, did) = cx.qpath_res(q, e.hir_id) {
                    if let DefKind::Static { mutability, .. } = kind {
                        match mutability {
                            Mutability::Mut => has_static_mut_ref = true,
                            Mutability::Not => {
                                if let Some(local_did) = did.as_local() {
                                    let owner_id = rustc_hir::OwnerId { def_id: local_did };
                                    let attrs = cx.tcx.hir_attrs(rustc_hir::HirId::from(owner_id));
                                    if rvs_has_attr(attrs, "thread_local") {
                                        has_thread_local_ref = true;
                                    }
                                }
                                has_static_ref = true;
                            }
                        }
                    }
                }
            }
        });

        rvs_walk_closures(cx.tcx, body.value, |e| {
            match &e.kind {
                ExprKind::Call(func, _) => {
                    if let ExprKind::Path(ref q) = func.kind {
                        if let Res::Def(k, did) = cx.qpath_res(q, func.hir_id) {
                            if matches!(k, DefKind::Fn | DefKind::AssocFn | DefKind::Variant) {
                                calls.insert(rvs_def_path(cx, did));
                            }
                        }
                    }
                }
                ExprKind::MethodCall(..) => {
                    let owner = e.hir_id.owner.def_id;
                    let tck = cx.tcx.typeck(owner);
                    if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                        calls.insert(rvs_def_path(cx, did));
                    }
                }
                // Capture function references passed as arguments (e.g. `&func` passed
                // to a `&dyn Fn` parameter).  These are not calls themselves, but the
                // referenced function will eventually be called through dynamic dispatch.
                // Recording them as edges allows capability propagation through dyn Fn.
                ExprKind::AddrOf(_, _, inner) => {
                    if let ExprKind::Path(ref q) = inner.kind {
                        if let Res::Def(k, did) = cx.qpath_res(q, inner.hir_id) {
                            if matches!(k, DefKind::Fn | DefKind::AssocFn) {
                                calls.insert(rvs_def_path(cx, did));
                            }
                        }
                    }
                }
                _ => {}
            }
        });

        let has_async = sig.header.asyncness.is_async();
        let is_unsafe_fn = matches!(
            sig.header.safety,
            rustc_hir::HeaderSafety::Normal(Safety::Unsafe)
        );
        let has_mut_param = rvs_has_mutable_params(sig, body);
        let has_unsafe_block = rvs_scan_unsafe(cx.tcx, body);

        let entry = self
            .callgraph
            .entry(caller_path)
            .or_insert_with(|| FnBehavior {
                calls: BTreeSet::new(),
                has_async,
                has_unsafe_block,
                is_unsafe_fn,
                has_mut_param,
                has_static_ref,
                has_static_mut_ref,
                has_thread_local_ref,
                is_trait_impl,
            });
        for callee in calls {
            entry.calls.insert(callee);
        }
    }
}

// ─── Type ident collection ───────────────────────────────────────────────

fn rvs_collect_type_idents_M(ty: &rustc_hir::Ty<'_>, out: &mut HashSet<String>) {
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

// ─── Per-item checks ─────────────────────────────────────────────────────

fn rvs_check_missing_doc_S(
    cx: &LateContext<'_>,
    name: &str,
    span: Span,
    attrs: &[rustc_hir::Attribute],
    is_pub: bool,
) {
    if !is_pub {
        return;
    }
    if !name.starts_with("rvs_") {
        return;
    }
    if rvs_has_attr(attrs, "test") {
        return;
    }
    if !rvs_has_any_doc(attrs) {
        cx.emit_span_lint(
            RVS_MISSING_DOC,
            span,
            Msg::new(span, format!("pub fn '{name}' missing /// doc comment")),
        );
    }
}

fn rvs_check_missing_safety_doc_S(
    cx: &LateContext<'_>,
    hir_id: HirId,
    span: Span,
    safety: &rustc_hir::HeaderSafety,
) {
    if !matches!(safety, rustc_hir::HeaderSafety::Normal(Safety::Unsafe)) {
        return;
    }
    if !rvs_has_doc_section(cx, hir_id, "Safety") {
        cx.emit_span_lint(
            RVS_MISSING_SAFETY_DOC,
            span,
            Msg::new(span, "unsafe fn missing /// # Safety"),
        );
    }
}

fn rvs_check_borrowed_fields_S(cx: &LateContext<'_>, fields: &[rustc_hir::FieldDef<'_>]) {
    for f in fields {
        if let TyKind::Ref(_, mt) = &f.ty.kind {
            if mt.mutbl == Mutability::Not {
                if let Some(name) = rvs_ty_last_ident(mt.ty) {
                    if BORROWED_TYPES.contains(&name.as_str()) {
                        let better = match name.as_str() {
                            "String" => "&str",
                            "Vec" => "&[T]",
                            "Box" => "&T",
                            _ => continue,
                        };
                        cx.emit_span_lint(
                            RVS_BORROWED_PARAM,
                            f.ty.span,
                            Msg::new(f.ty.span, format!("&{name} field — use {better} instead")),
                        );
                    }
                }
            }
        }
    }
}

fn rvs_check_todo_comments_source_S(cx: &LateContext<'_>, span: Span) {
    let source_map = cx.tcx.sess.source_map();
    if let Ok(src) = source_map.span_to_snippet(span) {
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                let lower = trimmed.to_ascii_lowercase();
                if lower.contains("todo") || lower.contains("fixme") {
                    cx.emit_span_lint(
                        RVS_TODO_COMMENT,
                        span,
                        Msg::new(span, "TODO/FIXME comment found"),
                    );
                    return;
                }
            }
        }
    }
}

// ─── Utility ─────────────────────────────────────────────────────────────

fn rvs_valid_test(n: &str) -> bool {
    let Some(r) = n.strip_prefix("test_") else {
        return false;
    };
    r.len() > 9
        && r[..8].chars().all(|c| c.is_ascii_digit())
        && r.as_bytes()[8] == b'_'
        && r[9..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}
