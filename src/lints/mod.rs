#![allow(
    clippy::all,
    reason = "lint pass glue code mirrors rustc internals and produces noisy style warnings"
)]
#![allow(
    internal_features,
    reason = "rustc_private integration requires compiler internal features"
)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rustc_lint::{LateContext, LateLintPass, LintPass};
use rustc_session::declare_tool_lint;
use rustc_span::{DUMMY_SP, Span};

use crate::capability::{CapabilityPolicy, ParsedFunctionName};
use crate::capsmap::CapsMap;
use crate::symbols::{CrateName, DefPath};

mod body;
mod caps;
mod ctx;
mod msg;
mod node;
mod test_quality;
mod utils;

pub use crate::artifacts::FnGraph;

use body::{catch_unwind, debug_assert, empty_fn, error_swallow, reflection, spawn, stub_macro};
use caps::callgraph;
use ctx::{FnCheckData, FnSubject};
use node::{
    banned_import, borrowed_param, catch_all_error, consumed_arg, dead_code, deny_warnings,
    deref_polymorphism, missing_allow, missing_debug_derive, missing_doc, missing_safety_doc,
    port_traits, test_name_format, todo_comment, validate,
};

// ─── Lint declarations ───────────────────────────────────────────────────

macro_rules! rvs_declare_lints {
    ($(($name:ident, $level:ident, $desc:expr)),+ $(,)?) => {
        $(declare_tool_lint! { pub rivus::$name, $level, $desc })+

        pub static RIVUS_LINTS: &[&rustc_lint::Lint] = &[$($name),+];
    };
}

rvs_declare_lints!(
    (RVS_CALL_VIOLATION, Deny, "capability call chain violation"),
    (
        RVS_STATIC_REF,
        Deny,
        "static/thread_local reference without capability"
    ),
    (RVS_STUB_MACRO, Deny, "todo!/unimplemented!() stub"),
    (RVS_EMPTY_FN, Deny, "empty function body"),
    (
        RVS_MISSING_DEBUG_ASSERT,
        Warn,
        "primitive numeric parameter without debug_assert!"
    ),
    (
        RVS_MISSING_ALLOW,
        Warn,
        "rvs_ function with uppercase suffix but no #[allow(non_snake_case)]"
    ),
    (RVS_NON_RVS_FN, Warn, "function missing rvs_ prefix"),
    (
        RVS_CONTRACT_MISMATCH,
        Warn,
        "Port method name does not match inferred public contract"
    ),
    (
        RVS_UNKNOWN_CALLEE,
        Warn,
        "call to function neither rvs_-prefixed nor in capsmap"
    ),
    (
        RVS_MISSING_MUTABLE,
        Warn,
        "function has &mut param but suffix lacks M"
    ),
    (RVS_MISSING_ASYNC, Warn, "async fn but suffix lacks A"),
    (RVS_MISSING_UNSAFE, Warn, "unsafe code but suffix lacks U"),
    (
        RVS_MISSING_SIDE_EFFECT,
        Warn,
        "reads static but suffix lacks S"
    ),
    (
        RVS_MISSING_THREAD_LOCAL,
        Warn,
        "reads thread_local! but suffix lacks T"
    ),
    (
        RVS_NON_ALPHABETICAL_SUFFIX,
        Warn,
        "capability suffix letters not in alphabetical order"
    ),
    (
        RVS_DUPLICATE_SUFFIX,
        Warn,
        "duplicate letter in capability suffix"
    ),
    (
        RVS_UNKNOWN_SUFFIX_LETTER,
        Warn,
        "suffix contains unrecognized capability letters"
    ),
    (RVS_SPAWN_WARNING, Warn, "unstructured spawn"),
    (
        RVS_DEAD_CODE,
        Warn,
        "rvs_ function marked #[allow(dead_code)] or #[allow(unused)]"
    ),
    (
        RVS_TEST_NAME_FORMAT,
        Warn,
        "test name does not match format"
    ),
    (RVS_BANNED_IMPORT, Warn, "import of banned crate"),
    (
        RVS_MISSING_DEBUG_DERIVE,
        Warn,
        "pub struct/enum missing #[derive(Debug)]"
    ),
    (
        RVS_ERROR_SWALLOW,
        Warn,
        ".ok(), .unwrap_or_default(), or drop(Result) discards error information"
    ),
    (
        RVS_CATCH_UNWIND,
        Warn,
        "catch_unwind — fix panic source instead"
    ),
    (
        RVS_REFLECTION_USAGE,
        Warn,
        "std::any::Any/type_name/type_id — use trait dispatch"
    ),
    (
        RVS_BORROWED_PARAM,
        Warn,
        "&String/&Vec<T>/&Box<T> — use &str/&[T]/&T"
    ),
    (RVS_INTO_IMPL, Warn, "impl Into<T> — implement From instead"),
    (
        RVS_DEREF_POLYMORPHISM,
        Warn,
        "impl Deref — use composition instead"
    ),
    (
        RVS_DENY_WARNINGS,
        Warn,
        "#![deny(warnings)] — use named lints"
    ),
    (RVS_WILDCARD_IMPORT, Warn, "use xxx::*; wildcard import"),
    (
        RVS_MISSING_DOC,
        Warn,
        "pub fn/method missing /// doc comment"
    ),
    (
        RVS_MISSING_SAFETY_DOC,
        Warn,
        "unsafe fn missing /// # Safety"
    ),
    (
        RVS_CATCH_ALL_ERROR_VARIANT,
        Warn,
        "error enum has Unknown/Other catch-all variant"
    ),
    (
        RVS_VALIDATE_RETURNS_UNIT,
        Warn,
        "validate/check/verify returns Result<(),E> — use TryFrom"
    ),
    (
        RVS_CONSUMED_ARG_ON_ERROR,
        Warn,
        "owned param consumed but not preserved in error type"
    ),
    (RVS_TODO_COMMENT, Warn, "// TODO or // FIXME comment"),
    (
        RVS_MISSING_TEST_OUTPUT,
        Warn,
        "test missing test_out/{name}.out snapshot"
    ),
    (RVS_DUPLICATE_TEST, Warn, "duplicate test function name"),
    (
        RVS_UNTESTED_GOOD_FN,
        Warn,
        "good function not called by any test"
    ),
    (
        RVS_UNTESTED_OK_FN,
        Warn,
        "ok function (ABMP subset, mock-testable) not called by any test"
    ),
);

macro_rules! rvs_fn_check_data {
    ($pass:expr) => {
        FnCheckData {
            good_fns: &mut $pass.good_fns,
            ok_fns: &mut $pass.ok_fns,
            callgraph: &mut $pass.callgraph,
            diagnostic_spans: &mut $pass.diagnostic_spans,
            collect_caps_facts: $pass.collect_caps_facts,
            should_emit_lints: $pass.should_emit_lints,
        }
    };
}

// ─── Lint pass ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RivusLintPass {
    capsmap: Option<CapsMap>,
    test_names: BTreeMap<String, Vec<ctx::TestSite>>,
    good_fns: Vec<ctx::CoverageFn>,
    ok_fns: Vec<ctx::CoverageFn>,
    test_calls: HashSet<ctx::TestCallTarget>,
    callgraph: FnGraph,
    diagnostic_spans: BTreeMap<DefPath, (rustc_hir::HirId, Span)>,
    done_crate_level: bool,
    collect_callgraph: bool,
    collect_caps_facts: bool,
    should_emit_lints: bool,
    should_emit_caps_report: bool,
    test_fn_names: HashSet<String>,
    banned_import_statements: HashSet<(rustc_span::StableSourceFileId, u32, String)>,
    untested_functions: Option<BTreeSet<crate::artifacts::FunctionIdentity>>,
    untested_functions_error: Option<String>,
}

impl RivusLintPass {
    /// Create a lint pass configured from the current process environment.
    pub fn rvs_new_BIS() -> Self {
        let collect_callgraph = crate::rvs_env_flag_is_one_BS("RIVUS_CALLGRAPH");
        let offline_caps_check = crate::rvs_env_flag_is_one_BS("RIVUS_OFFLINE_CAPS");
        let should_emit_caps_report = !collect_callgraph && !offline_caps_check;
        let (untested_functions, untested_functions_error) = match rvs_load_untested_functions_BIS()
        {
            Ok(functions) => (functions, None),
            Err(error) => (None, Some(error)),
        };
        Self {
            capsmap: None,
            test_names: BTreeMap::new(),
            good_fns: Vec::new(),
            ok_fns: Vec::new(),
            test_calls: HashSet::new(),
            callgraph: FnGraph::rvs_new(),
            diagnostic_spans: BTreeMap::new(),
            done_crate_level: false,
            collect_callgraph,
            collect_caps_facts: collect_callgraph || should_emit_caps_report,
            should_emit_lints: !collect_callgraph,
            should_emit_caps_report,
            test_fn_names: HashSet::new(),
            banned_import_statements: HashSet::new(),
            untested_functions,
            untested_functions_error,
        }
    }

    fn rvs_ensure_capsmap_BIMS(&mut self) -> Result<(), String> {
        if self.capsmap.is_some() {
            return Ok(());
        }
        let path = std::env::var_os("RIVUS_CAPSMAP").map(std::path::PathBuf::from);
        self.capsmap = Some(rvs_load_capsmap_path_BIS(path.as_deref())?);
        Ok(())
    }
}

fn rvs_load_capsmap_path_BIS(path: Option<&std::path::Path>) -> Result<CapsMap, String> {
    match path {
        Some(path) => CapsMap::rvs_load_BIS(path).map_err(|error| error.to_string()),
        None => Ok(CapsMap::rvs_new()),
    }
}

fn rvs_fulfill_collection_expectations_S(
    cx: &LateContext<'_>,
    hir_id: rustc_hir::HirId,
    span: Span,
) {
    for lint in RIVUS_LINTS {
        if cx.tcx.lint_level_at_node(lint, hir_id).level.as_str() == "expect" {
            msg::rvs_emit_node_span_lint_S(
                cx,
                lint,
                hir_id,
                span,
                "collection-only expectation marker",
            );
        }
    }
}

fn rvs_load_untested_functions_BIS()
-> Result<Option<BTreeSet<crate::artifacts::FunctionIdentity>>, String> {
    let Some(path) = std::env::var_os("RIVUS_UNTESTED_PATHS") else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(path);
    let json = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read untested-function selection {}: {error}",
            path.display()
        )
    })?;
    crate::artifacts::rvs_parse_function_identities_json_S(&json)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
fn rvs_env_flag_value_enabled(value: Option<&str>) -> bool {
    matches!(value, Some("1"))
}

impl Default for RivusLintPass {
    fn default() -> Self {
        Self::rvs_new_BIS()
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
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, rustc_hir::CRATE_HIR_ID, DUMMY_SP);
        }
        if let Some(error) = self.untested_functions_error.take() {
            cx.tcx.dcx().err(error);
        }
        if self.should_emit_caps_report {
            if let Err(error) = self.rvs_ensure_capsmap_BIMS() {
                cx.tcx.dcx().err(format!("failed to load capsmap: {error}"));
                self.should_emit_caps_report = false;
            }
        }

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
                                let test_name = ct.name.as_str();
                                self.test_fn_names.insert(test_name.to_string());
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

        if self.should_emit_caps_report {
            let local_crate_names = BTreeSet::from([CrateName::rvs_from_manifest_name(
                cx.tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).as_str(),
            )]);
            let caps = self
                .capsmap
                .as_ref()
                .expect("never: direct caps report loads capsmap in check_crate");
            let report = crate::offline_caps::rvs_check_offline_caps(
                &self.callgraph,
                caps,
                &local_crate_names,
            );
            rvs_emit_offline_caps_diagnostics_S(cx, &report, &self.diagnostic_spans);
        }

        test_quality::rvs_check_crate_post_BIMS(
            cx,
            &self.test_names,
            &self.good_fns,
            &self.ok_fns,
            &self.test_calls,
            self.untested_functions.as_ref(),
            &self.callgraph,
            self.collect_callgraph,
            self.should_emit_caps_report,
        );
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::Item<'tcx>) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, item.hir_id(), item.span);
        }
        let mut data = rvs_fn_check_data!(self);
        rvs_check_item_MS(
            cx,
            item,
            &self.test_fn_names,
            &mut self.test_names,
            &mut self.test_calls,
            &mut self.banned_import_statements,
            &mut data,
        );
    }

    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        impl_item: &'tcx rustc_hir::ImplItem<'tcx>,
    ) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, impl_item.hir_id(), impl_item.span);
        }
        let mut data = rvs_fn_check_data!(self);
        rvs_check_impl_item_MS(
            cx,
            impl_item,
            &self.test_fn_names,
            &mut self.test_names,
            &mut self.test_calls,
            &mut data,
        );
    }

    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx rustc_hir::TraitItem<'tcx>,
    ) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, trait_item.hir_id(), trait_item.span);
        }
        let mut data = rvs_fn_check_data!(self);
        rvs_check_trait_item_MS(cx, trait_item, &mut data);
    }

    fn check_stmt(&mut self, cx: &LateContext<'tcx>, statement: &'tcx rustc_hir::Stmt<'tcx>) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, statement.hir_id, statement.span);
        }
    }

    fn check_body(&mut self, cx: &LateContext<'tcx>, body: &rustc_hir::Body<'tcx>) {
        if self.collect_callgraph {
            for param in body.params {
                rvs_fulfill_collection_expectations_S(cx, param.hir_id, param.span);
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expression: &'tcx rustc_hir::Expr<'tcx>) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, expression.hir_id, expression.span);
        }
    }

    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx rustc_hir::FieldDef<'tcx>) {
        if self.collect_callgraph {
            rvs_fulfill_collection_expectations_S(cx, field.hir_id, field.span);
        }
    }
}

// ─── Dispatch functions ──────────────────────────────────────────────────

fn rvs_emit_offline_caps_diagnostics_S(
    cx: &LateContext<'_>,
    report: &crate::offline_caps::OfflineCapsReport,
    spans: &BTreeMap<DefPath, (rustc_hir::HirId, Span)>,
) {
    use crate::inference::FnContractMismatchKind;
    use crate::offline_caps::OfflineCapsKind;

    for diagnostic in &report.diagnostics {
        let lint = match diagnostic.kind {
            OfflineCapsKind::CallViolation => RVS_CALL_VIOLATION,
            OfflineCapsKind::Contract(kind) => match kind {
                FnContractMismatchKind::MissingAsync => RVS_MISSING_ASYNC,
                FnContractMismatchKind::MissingBlocking
                | FnContractMismatchKind::MissingIo
                | FnContractMismatchKind::MissingPort
                | FnContractMismatchKind::NameMismatch => RVS_CONTRACT_MISMATCH,
                FnContractMismatchKind::MissingMutable => RVS_MISSING_MUTABLE,
                FnContractMismatchKind::MissingRvsPrefix => RVS_NON_RVS_FN,
                FnContractMismatchKind::MissingSideEffect => RVS_MISSING_SIDE_EFFECT,
                FnContractMismatchKind::MissingThreadLocal => RVS_MISSING_THREAD_LOCAL,
                FnContractMismatchKind::MissingUnsafe => RVS_MISSING_UNSAFE,
            },
            OfflineCapsKind::DuplicateSuffix => RVS_DUPLICATE_SUFFIX,
            OfflineCapsKind::NonAlphabeticalSuffix => RVS_NON_ALPHABETICAL_SUFFIX,
            OfflineCapsKind::StaticRefRequiresCaps => RVS_STATIC_REF,
            OfflineCapsKind::UnknownCallee => RVS_UNKNOWN_CALLEE,
            OfflineCapsKind::UnknownSuffixLetter => RVS_UNKNOWN_SUFFIX_LETTER,
        };
        let message = if diagnostic.details.is_empty() {
            diagnostic.message.clone()
        } else {
            format!("{}; {}", diagnostic.message, diagnostic.details.join("; "))
        };
        for anchor in &diagnostic.span_anchors {
            let Some((hir_id, span)) = spans.get(anchor).copied() else {
                continue;
            };
            msg::rvs_emit_node_span_lint_S(cx, lint, hir_id, span, message.clone());
        }
    }
}

/// Dispatches to fn-level checks for free functions, inherent impl methods,
/// and trait impl methods.
fn rvs_run_fn_checks_MS<'tcx>(
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
    data: &mut FnCheckData<'_>,
) {
    let name = subject.rvs_name();
    let attrs = cx.tcx.hir_attrs(subject.hir_id);
    let parsed_name = ParsedFunctionName::rvs_parse(name);
    let has_rvs_prefix = parsed_name.rvs_has_rvs_prefix();
    if has_rvs_prefix || subject.is_port_method {
        let effective_caps = if subject.is_port_method {
            CapabilityPolicy::rvs_port_method_caps()
        } else {
            parsed_name.rvs_known_caps().clone()
        };

        let is_stub = stub_macro::rvs_check_fn_S(cx, subject.body_facts, subject.span);
        empty_fn::rvs_check_fn_MS(cx, subject.body, subject.span, subject.has_body, is_stub);
        if has_rvs_prefix {
            missing_allow::rvs_check_fn_S(
                cx,
                subject.hir_id,
                subject.span,
                parsed_name.rvs_raw_suffix().unwrap_or(""),
            );
        }
        dead_code::rvs_check_fn_S(cx, attrs, subject.span);

        // Spawn, reflection, catch_unwind, error swallow detection
        spawn::rvs_check_fn_S(cx, subject.body_facts, subject.is_test);
        reflection::rvs_check_fn_S(cx, subject.body_facts);
        catch_unwind::rvs_check_fn_S(cx, subject.body_facts);
        error_swallow::rvs_check_fn_S(cx, subject.body_facts);

        if subject.has_body && !is_stub {
            debug_assert::rvs_check_fn_MS(cx, subject.body, subject.body_facts);
            borrowed_param::rvs_check_fn_params_S(cx, subject.sig, subject.body.params);
            consumed_arg::rvs_check_fn_MS(cx, subject.sig, subject.body.params, name);
            validate::rvs_check_fn_S(cx, name, subject.sig);
        }

        let is_good = CapabilityPolicy::rvs_is_good(&effective_caps);
        let coverage_fns = if is_good {
            Some(&mut *data.good_fns)
        } else if CapabilityPolicy::rvs_is_ok(&effective_caps) && !subject.is_trait_impl {
            Some(&mut *data.ok_fns)
        } else {
            None
        };
        if has_rvs_prefix
            && data.should_emit_lints
            && !subject.is_test
            && !utils::rvs_has_allow(attrs, "dead_code")
            && !utils::rvs_has_allow(attrs, "unused")
            && let Some(coverage_fns) = coverage_fns
        {
            coverage_fns.push(ctx::CoverageFn {
                identity: crate::artifacts::FunctionIdentity {
                    crate_id: cx
                        .tcx
                        .stable_crate_id(subject.hir_id.owner.def_id.to_def_id().krate)
                        .as_u64(),
                    def_path: DefPath::rvs_new(utils::rvs_def_path(
                        cx,
                        subject.hir_id.owner.def_id.to_def_id(),
                    )),
                },
                name: name.to_string(),
                hir_id: subject.hir_id,
                span: subject.span,
            });
        }
    }
    test_name_format::rvs_check_fn_S(cx, name, subject.span, subject.is_test);
}

fn rvs_run_body_fn_pipeline_MS<'tcx, F>(
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
    data: &mut FnCheckData<'_>,
    should_check_fn: bool,
    after_checks: F,
) where
    F: FnOnce(),
{
    if should_check_fn {
        rvs_run_fn_checks_MS(cx, subject, data);
        todo_comment::rvs_check_fn_S(cx, subject.span);
        after_checks();
    }
    if data.collect_caps_facts {
        let def_path = callgraph::rvs_collect_callgraph_for_item_M(data.callgraph, cx, subject);
        data.diagnostic_spans
            .insert(def_path, (subject.hir_id, subject.span));
    }
}

/// Check free-fn / struct / enum / use / impl items.
fn rvs_check_item_MS<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx rustc_hir::Item<'tcx>,
    test_fn_names: &HashSet<String>,
    test_names: &mut BTreeMap<String, Vec<ctx::TestSite>>,
    test_calls: &mut HashSet<ctx::TestCallTarget>,
    banned_import_statements: &mut HashSet<(rustc_span::StableSourceFileId, u32, String)>,
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
            let body_facts = body::rvs_collect_body_facts_M(cx, body);
            let attrs = cx.tcx.hir_attrs(item.hir_id());
            let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
            let subject = FnSubject::rvs_body(
                *ident,
                item.hir_id(),
                item.span,
                sig,
                body,
                &body_facts,
                *has_body,
                is_test,
                false,
                false,
            );
            rvs_run_body_fn_pipeline_MS(cx, &subject, data, data.should_emit_lints, || {
                if is_test {
                    test_names
                        .entry(name.to_string())
                        .or_default()
                        .push(ctx::TestSite {
                            hir_id: item.hir_id(),
                            span: item.span,
                        });
                    body::collector::rvs_collect_test_calls_M(&body_facts, test_calls);
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
            });
        }
        ItemKind::Use(path, use_kind) => {
            if data.should_emit_lints {
                banned_import::rvs_check_item_MS(
                    cx,
                    item,
                    path,
                    *use_kind,
                    banned_import_statements,
                );
            }
        }
        ItemKind::ExternCrate(..) => {
            if data.should_emit_lints {
                banned_import::rvs_check_extern_crate_S(cx, item);
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
                match data_fields {
                    VariantData::Struct { fields, .. } | VariantData::Tuple(fields, ..) => {
                        borrowed_param::rvs_check_borrowed_fields_S(cx, fields);
                    }
                    VariantData::Unit(..) => {}
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
fn rvs_check_impl_item_MS<'tcx>(
    cx: &LateContext<'tcx>,
    impl_item: &'tcx rustc_hir::ImplItem<'tcx>,
    test_fn_names: &HashSet<String>,
    test_names: &mut BTreeMap<String, Vec<ctx::TestSite>>,
    test_calls: &mut HashSet<ctx::TestCallTarget>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{Item, ItemKind};

    if let rustc_hir::ImplItemKind::Fn(sig, body_id) = &impl_item.kind {
        let parent = cx.tcx.hir_get_parent_item(impl_item.hir_id());
        let parent_node = cx.tcx.hir_owner_node(parent);
        let (is_trait_impl, is_port_method) = match parent_node {
            rustc_hir::OwnerNode::Item(Item {
                kind: ItemKind::Impl(imp),
                ..
            }) => match &imp.of_trait {
                Some(trait_ref) => (
                    true,
                    trait_ref.trait_ref.trait_def_id().is_some_and(|trait_did| {
                        port_traits::rvs_is_local_port_trait_S(cx, trait_did)
                    }),
                ),
                None => (false, false),
            },
            _ => (false, false),
        };
        let name = impl_item.ident.name.as_str();
        let body = cx.tcx.hir_body(*body_id);
        let body_facts = body::rvs_collect_body_facts_M(cx, body);
        let attrs = cx.tcx.hir_attrs(impl_item.hir_id());
        let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
        let is_pub = !is_trait_impl && cx.tcx.visibility(impl_item.owner_id.def_id).is_public();
        let subject = FnSubject::rvs_body(
            impl_item.ident,
            impl_item.hir_id(),
            impl_item.span,
            sig,
            body,
            &body_facts,
            true,
            is_test,
            is_trait_impl,
            is_port_method,
        );
        // Port trait methods are checked (with P capability auto-assigned),
        // even though other trait impl methods are skipped.
        let should_check_fn = data.should_emit_lints && (!is_trait_impl || is_port_method);
        rvs_run_body_fn_pipeline_MS(cx, &subject, data, should_check_fn, || {
            if is_test {
                test_names
                    .entry(name.to_string())
                    .or_default()
                    .push(ctx::TestSite {
                        hir_id: impl_item.hir_id(),
                        span: impl_item.span,
                    });
                body::collector::rvs_collect_test_calls_M(&body_facts, test_calls);
            }
            if !is_test && is_pub {
                missing_doc::rvs_check_fn_S(cx, name, impl_item.span, attrs, true);
            }
            missing_safety_doc::rvs_check_fn_S(
                cx,
                impl_item.hir_id(),
                impl_item.span,
                &sig.header.safety,
            );
        });
        if data.should_emit_lints && !should_check_fn {
            todo_comment::rvs_check_fn_S(cx, impl_item.span);
        }
    }
}

/// Check trait method (provided or required).
fn rvs_check_trait_item_MS<'tcx>(
    cx: &LateContext<'tcx>,
    trait_item: &'tcx rustc_hir::TraitItem<'tcx>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{TraitFn, TraitItemKind};

    // Determine if this trait item belongs to a Port trait.
    let parent = cx.tcx.hir_get_parent_item(trait_item.hir_id());
    let parent_def_id = parent.def_id.to_def_id();
    let is_port_trait = port_traits::rvs_is_local_port_trait_S(cx, parent_def_id);
    let is_pub = cx.tcx.visibility(parent_def_id).is_public();
    let attrs = cx.tcx.hir_attrs(trait_item.hir_id());

    match &trait_item.kind {
        TraitItemKind::Fn(sig, TraitFn::Provided(body_id)) => {
            let body = cx.tcx.hir_body(*body_id);
            let body_facts = body::rvs_collect_body_facts_M(cx, body);
            let subject = FnSubject::rvs_body(
                trait_item.ident,
                trait_item.hir_id(),
                trait_item.span,
                sig,
                body,
                &body_facts,
                true,
                false,
                false,
                is_port_trait,
            );
            rvs_run_body_fn_pipeline_MS(cx, &subject, data, data.should_emit_lints, || {
                missing_doc::rvs_check_fn_S(
                    cx,
                    trait_item.ident.name.as_str(),
                    trait_item.span,
                    attrs,
                    is_pub,
                );
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    trait_item.hir_id(),
                    trait_item.span,
                    &sig.header.safety,
                );
            });
        }
        TraitItemKind::Fn(sig, TraitFn::Required(_)) => {
            if data.should_emit_lints {
                let name = trait_item.ident.name.as_str();
                let parsed_name = ParsedFunctionName::rvs_parse(name);
                if parsed_name.rvs_has_rvs_prefix() {
                    missing_allow::rvs_check_fn_S(
                        cx,
                        trait_item.hir_id(),
                        trait_item.span,
                        parsed_name.rvs_raw_suffix().unwrap_or(""),
                    );
                }
                missing_doc::rvs_check_fn_S(cx, name, trait_item.span, attrs, is_pub);
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    trait_item.hir_id(),
                    trait_item.span,
                    &sig.header.safety,
                );
            }
            // Required methods (no body) — collect signature info for callgraph.
            if data.collect_caps_facts {
                let def_path = callgraph::rvs_collect_callgraph_for_signature_M(
                    data.callgraph,
                    cx,
                    trait_item.hir_id(),
                    trait_item.ident,
                    sig,
                    false,
                    is_port_trait,
                );
                data.diagnostic_spans
                    .insert(def_path, (trait_item.hir_id(), trait_item.span));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260714_explicit_capsmap_load_failure_is_fatal() {
        let path =
            std::env::temp_dir().join("test_20260714_explicit_capsmap_load_failure_is_fatal");
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("seed"), "invalid capsmap line\n").unwrap();

        let result = rvs_load_capsmap_path_BIS(Some(&path));
        let output = match &result {
            Ok(_) => "ok\n".to_string(),
            Err(error) => {
                format!("error={error}\n").replace(path.to_string_lossy().as_ref(), "CAPSMAP_PATH")
            }
        };
        rvs_snapshot_BIS(
            "test_20260714_explicit_capsmap_load_failure_is_fatal",
            &output,
        );
        std::fs::remove_dir_all(&path).ok();

        assert!(result.is_err());
    }

    #[test]
    fn test_20260706_env_flag_value_requires_one() {
        let cases = [
            (None, false),
            (Some(""), false),
            (Some("0"), false),
            (Some("true"), false),
            (Some("1"), true),
        ];
        let output = cases
            .iter()
            .map(|(value, _)| format!("{value:?}={}", rvs_env_flag_value_enabled(*value)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260706_env_flag_value_requires_one", &output);

        for (value, expected) in cases {
            assert_eq!(rvs_env_flag_value_enabled(value), expected);
        }
    }
}
