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
use rustc_span::Span;

use crate::artifacts::{CallSiteIdentity, CrateProvenance, FunctionIdentity};
use crate::capability::{
    Capability, CapabilityFacts, CapabilityPolicy, CapabilitySet, ParsedFunctionName,
};
use crate::capsmap::CapsMap;
use crate::symbols::{CrateName, DefPath};

mod body;
mod caps;
mod ctx;
mod msg;
mod node;
mod ports;
mod test_quality;
mod utils;

pub use crate::artifacts::FnGraph;
pub(crate) use ports::{LintEnvironment, LintExecutionMode, RivusLintConfig};

use body::{catch_unwind, debug_assert, empty_fn, error_swallow, reflection, spawn, stub_macro};
use caps::callgraph;
use ctx::{FnCheckData, FnSubject};
use node::{
    banned_import, borrowed_param, catch_all_error, consumed_arg, data_struct, dead_code,
    deny_warnings, deref_polymorphism, implicit_execution, missing_allow, missing_debug_derive,
    missing_doc, missing_safety_doc, port_traits, test_name_format, todo_comment, validate,
};

// ─── Lint declarations ───────────────────────────────────────────────────

macro_rules! rvs_declare_lints {
    ($(($name:ident, $level:ident, $desc:expr)),+ $(,)?) => {
        $(declare_tool_lint! { pub rivus::$name, $level, $desc })+

        pub static RIVUS_LINTS: &[&rustc_lint::Lint] = &[$($name),+];

        /// Default levels parallel to `RIVUS_LINTS`, so tests can assert
        /// that offline diagnostic severity maps onto the lint level.
        #[cfg(test)]
        pub static RIVUS_LINT_LEVELS: &[rustc_lint::Level] =
            &[$(rustc_lint::Level::$level),+];
    };
}

rvs_declare_lints!(
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
        Deny,
        "function name does not match inferred public contract"
    ),
    (
        RVS_UNKNOWN_CALLEE,
        Warn,
        "call to function neither rvs_-prefixed nor in capsmap"
    ),
    (
        RVS_INCOMPLETE_CAPS_KNOWLEDGE,
        Warn,
        "call check relies on incomplete caps knowledge"
    ),
    (
        RVS_TRAIT_IMPL_OUTLIER,
        Warn,
        "trait implementation has capabilities outside the aggregate vote"
    ),
    (
        RVS_NON_SUFFIX_CAP_IN_SUFFIX,
        Deny,
        "suffix contains non-suffix capability A/C/U; those are measured from the signature or body facts"
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
        RVS_TESTS_IMPORT,
        Deny,
        "import of tests-module symbol from non-test code"
    ),
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
        "ok function (ABCMP subset, mock-testable) not called by any test"
    ),
    (
        RVS_UNSUPPORTED_INDIRECT_CALL,
        Warn,
        "call through function pointer or generic callable cannot be resolved in HIR"
    ),
    (
        RVS_UNSUPPORTED_IMPLICIT_EXECUTION,
        Deny,
        "implicit or opaque execution cannot be represented in the Rivus callgraph"
    ),
    (
        RVS_DATA_STRUCT_MUT_METHOD,
        Warn,
        "&mut self method on a struct whose fields are all externally visible; it is pure data — use free functions or hide the fields"
    ),
    (
        RVS_REDUNDANT_FIELD_ACCESSOR,
        Warn,
        "method only returning/borrowing a field that is already visible to every caller of the method"
    ),
);

macro_rules! rvs_fn_check_data {
    ($pass:expr) => {
        FnCheckData {
            good_fns: &mut $pass.good_fns,
            ok_fns: &mut $pass.ok_fns,
            callgraph: &mut $pass.callgraph,
            diagnostic_spans: &mut $pass.diagnostic_spans,
            diagnostic_call_spans: &mut $pass.diagnostic_call_spans,
            mode: $pass.mode,
            crate_provenance: $pass.crate_provenance,
        }
    };
}

// ─── Lint pass ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RivusLintPass<E: LintEnvironment> {
    capsmap: Option<CapsMap>,
    capsmap_error: Option<String>,
    test_names: BTreeMap<String, Vec<ctx::TestSite>>,
    good_fns: Vec<ctx::CoverageFn>,
    ok_fns: Vec<ctx::CoverageFn>,
    test_calls: HashSet<ctx::TestCallTarget>,
    callgraph: FnGraph,
    diagnostic_spans: BTreeMap<FunctionIdentity, (rustc_hir::HirId, Span)>,
    diagnostic_call_spans: BTreeMap<(FunctionIdentity, CallSiteIdentity), (rustc_hir::HirId, Span)>,
    done_crate_level: bool,
    mode: LintExecutionMode,
    test_fn_names: HashSet<String>,
    banned_import_statements: HashSet<(rustc_span::StableSourceFileId, u32, String)>,
    untested_functions:
        Option<BTreeMap<crate::artifacts::FunctionIdentity, crate::artifacts::CoverageLabel>>,
    untested_functions_error: Option<String>,
    offline_emissions: Vec<crate::offline_caps::OfflineCapsEmission>,
    offline_emissions_error: Option<String>,
    test_outputs: Option<BTreeSet<String>>,
    world: E::World,
    interpreter: std::marker::PhantomData<E>,
    ui_testing: bool,
    crate_provenance: CrateProvenance,
}

impl<E: LintEnvironment> RivusLintPass<E> {
    /// Build a lint pass from the typed driver configuration, retaining
    /// load failures as deferred diagnostics instead of failing eagerly.
    pub fn rvs_new(config: RivusLintConfig<E>) -> Self {
        let RivusLintConfig {
            mode,
            capsmap,
            untested_functions,
            offline_emissions,
            test_outputs,
            ui_testing,
            crate_provenance,
            world,
            interpreter,
        } = config;
        let (capsmap, capsmap_error) = match capsmap {
            Ok(capsmap) => (capsmap, None),
            Err(error) => (None, Some(error)),
        };
        let (untested_functions, untested_functions_error) = match untested_functions {
            Ok(functions) => (functions, None),
            Err(error) => (None, Some(error)),
        };
        let (offline_emissions, offline_emissions_error) = match offline_emissions {
            Ok(emissions) => (emissions, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        Self {
            capsmap,
            capsmap_error,
            test_names: BTreeMap::new(),
            good_fns: Vec::new(),
            ok_fns: Vec::new(),
            test_calls: HashSet::new(),
            callgraph: FnGraph::rvs_new(),
            diagnostic_spans: BTreeMap::new(),
            diagnostic_call_spans: BTreeMap::new(),
            done_crate_level: false,
            mode,
            test_fn_names: HashSet::new(),
            banned_import_statements: HashSet::new(),
            untested_functions,
            untested_functions_error,
            offline_emissions,
            offline_emissions_error,
            test_outputs,
            world,
            interpreter,
            ui_testing,
            crate_provenance,
        }
    }

    fn rvs_ensure_capsmap_M(&mut self) -> Result<(), String> {
        if self.capsmap.is_some() {
            return Ok(());
        }
        Err(self
            .capsmap_error
            .clone()
            .unwrap_or_else(|| "capsmap was not prepared by the environment adapter".to_string()))
    }
}

fn rvs_check_scoped_rivus_lint_attrs_S(cx: &LateContext<'_>, hir_id: rustc_hir::HirId, span: Span) {
    let level_syms: &[rustc_span::Symbol] = &[
        rustc_span::Symbol::intern("allow"),
        rustc_span::Symbol::intern("deny"),
        rustc_span::Symbol::intern("expect"),
        rustc_span::Symbol::intern("forbid"),
        rustc_span::Symbol::intern("warn"),
    ];
    let rivus_sym = rustc_span::Symbol::intern("rivus");
    for attr in cx.tcx.hir_attrs(hir_id) {
        let Some(name) = attr.name() else {
            continue;
        };
        if !level_syms.contains(&name) {
            continue;
        }
        let Some(items) = attr.meta_item_list() else {
            continue;
        };
        for item in items {
            let Some(mi) = item.meta_item() else {
                continue;
            };
            if mi
                .path
                .segments
                .first()
                .is_some_and(|seg| seg.ident.name == rivus_sym)
            {
                cx.tcx.dcx().span_err(
                    span,
                    "scoped Rivus lint levels are unsupported; configure Rivus lints only at crate root",
                );
                return;
            }
        }
    }
}

impl<E: LintEnvironment> LintPass for RivusLintPass<E> {
    fn name(&self) -> &'static str {
        "RivusLintPass"
    }
    fn get_lints(&self) -> Vec<&'static rustc_lint::Lint> {
        RIVUS_LINTS.to_vec()
    }
}

impl<'tcx, E: LintEnvironment> LateLintPass<'tcx> for RivusLintPass<E> {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if let Some(error) = self.untested_functions_error.take() {
            cx.tcx.dcx().err(error);
        }
        if let Some(error) = self.offline_emissions_error.take() {
            cx.tcx.dcx().err(error);
        }
        if self.mode.rvs_is_caps_report()
            && let Err(error) = self.rvs_ensure_capsmap_M()
        {
            cx.tcx.dcx().err(format!("failed to load capsmap: {error}"));
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

        if self.mode.rvs_should_emit_lints() {
            deny_warnings::rvs_check_crate_S(cx);
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        if self.done_crate_level {
            return;
        }
        self.done_crate_level = true;

        // A caps-report mode whose capsmap failed to load has already
        // reported the load error in check_crate; the report is skipped.
        if self.mode.rvs_is_caps_report() && self.capsmap.is_some() {
            let local_crate_names = BTreeSet::from([CrateName::rvs_from_manifest_name(
                cx.tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).as_str(),
            )]);
            let caps = self
                .capsmap
                .as_ref()
                .expect("never: caps report mode has a loaded capsmap");
            let report = crate::offline_caps::rvs_check_offline_caps(
                &self.callgraph,
                caps,
                &local_crate_names,
            );
            rvs_emit_offline_caps_diagnostics_MP::<E>(
                cx,
                &report,
                &self.callgraph,
                &self.diagnostic_spans,
                &self.diagnostic_call_spans,
                &mut self.world,
                self.ui_testing,
            );
        }
        rvs_emit_offline_caps_emissions_MPS::<E>(
            cx,
            &self.offline_emissions,
            &self.diagnostic_spans,
            &self.diagnostic_call_spans,
            true,
            &mut self.world,
            self.ui_testing,
        );

        test_quality::rvs_check_crate_post_MPS::<E>(
            cx,
            &self.test_names,
            &self.good_fns,
            &self.ok_fns,
            &self.test_calls,
            self.untested_functions.as_ref(),
            &self.callgraph,
            self.test_outputs.as_ref(),
            &mut self.world,
            self.mode.rvs_is_caps_report(),
            self.ui_testing,
        );
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::Item<'tcx>) {
        rvs_check_scoped_rivus_lint_attrs_S(cx, item.hir_id(), item.span);
        if let rustc_hir::ItemKind::Enum(_, _, enum_def) = &item.kind {
            for variant in enum_def.variants {
                rvs_check_scoped_rivus_lint_attrs_S(cx, variant.hir_id, variant.span);
            }
        }
        let mut data = rvs_fn_check_data!(self);
        rvs_check_item_BMS(
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
        rvs_check_scoped_rivus_lint_attrs_S(cx, impl_item.hir_id(), impl_item.span);
        let mut data = rvs_fn_check_data!(self);
        rvs_check_impl_item_BMS(
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
        rvs_check_scoped_rivus_lint_attrs_S(cx, trait_item.hir_id(), trait_item.span);
        let mut data = rvs_fn_check_data!(self);
        rvs_check_trait_item_BMS(cx, trait_item, &mut data);
    }

    fn check_stmt(&mut self, cx: &LateContext<'tcx>, statement: &'tcx rustc_hir::Stmt<'tcx>) {
        rvs_check_scoped_rivus_lint_attrs_S(cx, statement.hir_id, statement.span);
    }

    fn check_body(&mut self, cx: &LateContext<'tcx>, body: &rustc_hir::Body<'tcx>) {
        for param in body.params {
            rvs_check_scoped_rivus_lint_attrs_S(cx, param.hir_id, param.span);
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expression: &'tcx rustc_hir::Expr<'tcx>) {
        rvs_check_scoped_rivus_lint_attrs_S(cx, expression.hir_id, expression.span);
    }

    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx rustc_hir::FieldDef<'tcx>) {
        rvs_check_scoped_rivus_lint_attrs_S(cx, field.hir_id, field.span);
    }
}

// ─── Dispatch functions ──────────────────────────────────────────────────

fn rvs_emit_offline_caps_diagnostics_MP<E: LintEnvironment>(
    cx: &LateContext<'_>,
    report: &crate::offline_caps::OfflineCapsReport,
    graph: &FnGraph,
    spans: &BTreeMap<FunctionIdentity, (rustc_hir::HirId, Span)>,
    call_spans: &BTreeMap<(FunctionIdentity, CallSiteIdentity), (rustc_hir::HirId, Span)>,
    world: &mut E::World,
    ui_testing: bool,
) {
    rvs_emit_offline_caps_emissions_MPS::<E>(
        cx,
        &report.rvs_emissions(graph),
        spans,
        call_spans,
        false,
        world,
        ui_testing,
    );
}

fn rvs_emit_offline_caps_emissions_MPS<E: LintEnvironment>(
    cx: &LateContext<'_>,
    emissions: &[crate::offline_caps::OfflineCapsEmission],
    spans: &BTreeMap<FunctionIdentity, (rustc_hir::HirId, Span)>,
    call_spans: &BTreeMap<(FunctionIdentity, CallSiteIdentity), (rustc_hir::HirId, Span)>,
    acknowledge: bool,
    world: &mut E::World,
    ui_testing: bool,
) {
    for (emission_index, emission) in emissions.iter().enumerate() {
        if emission.lint == crate::offline_caps::OfflineCapsLint::IncompleteCapsKnowledge
            && ui_testing
        {
            continue;
        }
        let lint = rvs_offline_caps_lint_S(emission.lint);
        for (anchor_index, anchor) in emission.span_anchors.iter().enumerate() {
            let location = rvs_resolve_emission_location(anchor, spans, call_spans);
            let Some((hir_id, span)) = location else {
                continue;
            };
            msg::rvs_emit_node_span_lint_S(cx, lint, hir_id, span, emission.message.clone());
            if acknowledge
                && let Err(error) =
                    E::rvs_acknowledge_offline_emission_P(world, emission_index, anchor_index)
            {
                cx.tcx.dcx().err(error);
            }
        }
    }
}

fn rvs_resolve_emission_location<T: Copy>(
    anchor: &crate::offline_caps::OfflineCapsEmissionAnchor,
    spans: &BTreeMap<FunctionIdentity, T>,
    call_spans: &BTreeMap<(FunctionIdentity, CallSiteIdentity), T>,
) -> Option<T> {
    match &anchor.call_site {
        Some(call_site) => call_spans
            .get(&(anchor.identity.clone(), call_site.clone()))
            .copied(),
        None => spans.get(&anchor.identity).copied(),
    }
}

fn rvs_offline_caps_lint_S(
    lint: crate::offline_caps::OfflineCapsLint,
) -> &'static rustc_lint::Lint {
    use crate::offline_caps::OfflineCapsLint;

    match lint {
        OfflineCapsLint::ContractMismatch => RVS_CONTRACT_MISMATCH,
        OfflineCapsLint::DuplicateSuffix => RVS_DUPLICATE_SUFFIX,
        OfflineCapsLint::IncompleteCapsKnowledge => RVS_INCOMPLETE_CAPS_KNOWLEDGE,
        OfflineCapsLint::MissingRvsPrefix => RVS_NON_RVS_FN,
        OfflineCapsLint::NonAlphabeticalSuffix => RVS_NON_ALPHABETICAL_SUFFIX,
        OfflineCapsLint::NonSuffixCapInSuffix => RVS_NON_SUFFIX_CAP_IN_SUFFIX,
        OfflineCapsLint::TraitImplOutlier => RVS_TRAIT_IMPL_OUTLIER,
        OfflineCapsLint::UnknownCallee => RVS_UNKNOWN_CALLEE,
        OfflineCapsLint::UnknownSuffixLetter => RVS_UNKNOWN_SUFFIX_LETTER,
    }
}

fn rvs_check_unsupported_indirect_calls_S(cx: &LateContext<'_>, facts: &body::BodyFacts) {
    use crate::lints::utils::ObservationKind;
    for observation in &facts.call_observations {
        if observation.kind == ObservationKind::UnsupportedIndirect
            && !facts.unsupported_implicit_execution.iter().any(|site| {
                site.kind == body::ImplicitExecutionKind::ExplicitFnTraitCall
                    && site.hir_id == observation.hir_id
            })
        {
            msg::rvs_emit_node_span_lint_S(
                cx,
                RVS_UNSUPPORTED_INDIRECT_CALL,
                observation.hir_id,
                observation.span,
                "call through function pointer or generic callable — Rivus cannot resolve the target at HIR level",
            );
        }
    }
}

fn rvs_check_unsupported_implicit_execution_S(cx: &LateContext<'_>, facts: &body::BodyFacts) {
    for site in &facts.unsupported_implicit_execution {
        let message = match site.kind {
            body::ImplicitExecutionKind::ExplicitFnTraitCall => {
                "explicit Fn/FnMut/FnOnce invocation hides the callable target from Rivus"
            }
            body::ImplicitExecutionKind::InlineAsm => {
                "inline asm execution is opaque to Rivus capability analysis"
            }
        };
        msg::rvs_emit_node_span_lint_S(
            cx,
            RVS_UNSUPPORTED_IMPLICIT_EXECUTION,
            site.hir_id,
            site.span,
            message,
        );
    }
}

/// Diagnostic emission scope selected once by the HIR dispatcher from the
/// owner's identity, per the function-graph theory: project invariants fire
/// for both scopes, production diagnostics only for `Production`, and test
/// contracts only for real `#[test]` functions inside `Test`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticScope {
    Production,
    Test,
}

/// Test source: a `#[test]` function or an owner whose DefPath contains a
/// `tests` segment before the function-name segment. Nested bodies inherit
/// the enclosing owner's scope and never re-derive it.
fn rvs_diagnostic_scope_B(
    cx: &LateContext<'_>,
    def_id: rustc_span::def_id::DefId,
    is_test: bool,
) -> DiagnosticScope {
    let path = DefPath::rvs_new(utils::rvs_def_path_B(cx, def_id));
    if is_test || path.rvs_is_in_test_module() {
        DiagnosticScope::Test
    } else {
        DiagnosticScope::Production
    }
}

/// Dispatches to fn-level checks for free functions, inherent impl methods,
/// and trait impl methods.
fn rvs_run_fn_checks_S<'tcx>(cx: &LateContext<'tcx>, subject: &FnSubject<'_, 'tcx>) {
    // Trait impl methods (non-Port) keep their historical exemption from
    // fn-level production checks, including coverage registration; their
    // unfinished-marker feedback runs at the dispatch site instead.
    if subject.is_trait_impl && !subject.is_port_method {
        return;
    }
    let name = subject.rvs_name();
    let attrs = cx.tcx.hir_attrs(subject.hir_id);
    let parsed_name = ParsedFunctionName::rvs_parse(name);
    let has_rvs_prefix = parsed_name.rvs_has_rvs_prefix();
    if has_rvs_prefix || subject.is_port_method {
        let effective_caps = rvs_effective_caps(subject);

        let is_stub = stub_macro::rvs_check_fn_S(cx, subject.body_facts, subject.span);
        empty_fn::rvs_check_fn_S(cx, subject.body, subject.span, subject.has_body, is_stub);
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
            debug_assert::rvs_check_fn_S(cx, subject.body, subject.body_facts);
            borrowed_param::rvs_check_fn_params_S(cx, subject.sig, subject.body.params);
            consumed_arg::rvs_check_fn_S(cx, subject.sig, subject.body.params, name);
            validate::rvs_check_fn_S(cx, name, subject.sig, &effective_caps);
        }
    }
    test_name_format::rvs_check_fn_S(cx, name, subject.span, subject.is_test);
}

/// Semantic caps in direct mode: the signature/body facts plus the
/// structural Port marker. The name suffix is a read-only view and
/// contributes nothing; propagated capabilities only appear in the
/// offline engine, which owns call-edge closure.
fn rvs_effective_caps(subject: &FnSubject<'_, '_>) -> CapabilitySet {
    let facts = CapabilityFacts::rvs_from_signature(
        subject.sig,
        utils::rvs_has_mutable_params(subject.sig),
        subject.is_port_method,
    )
    .rvs_with_static_refs(
        subject.body_facts.has_static_ref,
        subject.body_facts.has_static_mut_ref,
        subject.body_facts.has_thread_local_ref,
    );
    let mut effective_caps = CapabilityPolicy::rvs_signature_caps(facts);
    if subject.is_port_method {
        effective_caps.rvs_insert_M(Capability::P);
    }
    effective_caps
}

/// Registers a good/ok coverage candidate for the untested selection.
/// Registration is collection, not emission: it also runs in replay
/// processes that no longer emit direct lints, so the selection can
/// anchor onto real spans.
fn rvs_register_coverage_candidate_BM<'tcx>(
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
    data: &mut FnCheckData<'_>,
) {
    if subject.is_trait_impl && !subject.is_port_method {
        return;
    }
    let name = subject.rvs_name();
    let parsed_name = ParsedFunctionName::rvs_parse(name);
    if !parsed_name.rvs_has_rvs_prefix() {
        return;
    }
    let attrs = cx.tcx.hir_attrs(subject.hir_id);
    if subject.is_test
        || utils::rvs_has_allow(attrs, "dead_code")
        || utils::rvs_has_allow(attrs, "unused")
    {
        return;
    }
    let effective_caps = rvs_effective_caps(subject);
    let coverage_fns = if CapabilityPolicy::rvs_is_good(&effective_caps) {
        Some(&mut *data.good_fns)
    } else if CapabilityPolicy::rvs_is_ok(&effective_caps) && !subject.is_trait_impl {
        Some(&mut *data.ok_fns)
    } else {
        None
    };
    if let Some(coverage_fns) = coverage_fns {
        coverage_fns.push(ctx::CoverageFn {
            identity: crate::artifacts::FunctionIdentity {
                crate_id: cx
                    .tcx
                    .stable_crate_id(subject.hir_id.owner.def_id.to_def_id().krate)
                    .as_u64(),
                def_path: DefPath::rvs_new(utils::rvs_def_path_B(
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

fn rvs_run_body_fn_pipeline_BMS<'tcx, F>(
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
    data: &mut FnCheckData<'_>,
    scope: DiagnosticScope,
    after_checks: F,
) where
    F: FnOnce(DiagnosticScope),
{
    if data.mode.rvs_should_emit_lints() {
        match scope {
            DiagnosticScope::Production => {
                rvs_run_fn_checks_S(cx, subject);
                if !(subject.is_trait_impl && !subject.is_port_method) {
                    todo_comment::rvs_check_fn_S(cx, subject.span);
                }
            }
            DiagnosticScope::Test => {
                test_name_format::rvs_check_fn_S(
                    cx,
                    subject.rvs_name(),
                    subject.span,
                    subject.is_test,
                );
            }
        }
        after_checks(scope);
        // Unsupported indirect calls are a production diagnostic; test
        // source keeps the observation for coverage analysis only.
        if scope == DiagnosticScope::Production {
            rvs_check_unsupported_indirect_calls_S(cx, subject.body_facts);
        }
        // Implicit execution is a project invariant and applies to both
        // scopes.
        rvs_check_unsupported_implicit_execution_S(cx, subject.body_facts);
    }
    // Coverage candidates are production source only, matching the
    // offline engine's is_coverage_candidate exclusion of test modules.
    if data.mode.rvs_registers_coverage_candidates() && scope == DiagnosticScope::Production {
        rvs_register_coverage_candidate_BM(cx, subject, data);
    }
    if data.mode.rvs_collect_caps_facts() {
        let collected = callgraph::rvs_collect_callgraph_for_item_BMS(
            data.callgraph,
            cx,
            subject,
            data.crate_provenance,
        );
        data.diagnostic_spans
            .insert(collected.caller.clone(), (subject.hir_id, subject.span));
        for call_site in collected.call_sites {
            data.diagnostic_call_spans.insert(
                (collected.caller.clone(), call_site.identity),
                (call_site.hir_id, call_site.span),
            );
        }
    }
}

/// Check free-fn / struct / enum / use / impl items.
fn rvs_check_item_BMS<'tcx>(
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
            let body_facts =
                body::rvs_collect_body_facts_B(cx, body, data.mode.rvs_should_emit_lints());
            let attrs = cx.tcx.hir_attrs(item.hir_id());
            let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
            let scope = rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), is_test);
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
            rvs_run_body_fn_pipeline_BMS(cx, &subject, data, scope, |scope| {
                ctx::rvs_record_test_site_M(
                    is_test,
                    name,
                    item.hir_id(),
                    item.span,
                    &body_facts,
                    test_names,
                    test_calls,
                );
                if scope == DiagnosticScope::Production {
                    let vis = cx.tcx.visibility(item.owner_id.def_id);
                    let is_pub = vis.is_public();
                    missing_doc::rvs_check_fn_S(cx, name, item.span, attrs, is_pub);
                    missing_safety_doc::rvs_check_fn_S(
                        cx,
                        item.hir_id(),
                        item.span,
                        &sig.header.safety,
                    );
                }
            });
        }
        ItemKind::Use(path, use_kind) => {
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
                banned_import::rvs_check_item_BMS(
                    cx,
                    item,
                    path,
                    *use_kind,
                    banned_import_statements,
                );
            }
        }
        ItemKind::ExternCrate(..) => {
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
                banned_import::rvs_check_extern_crate_S(cx, item);
            }
        }
        ItemKind::Enum(_, _, enum_def) => {
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
                missing_debug_derive::rvs_check_struct_or_enum_S(cx, item);
                catch_all_error::rvs_check_enum_S(cx, item, enum_def);
            }
        }
        ItemKind::Struct(_, _, data_fields) => {
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
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
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
                deref_polymorphism::rvs_check_impl_S(cx, item, imp);
                implicit_execution::rvs_check_impl_S(cx, item, imp);
            }
        }
        _ => {}
    }
}

/// Check inherent impl method.
fn rvs_check_impl_item_BMS<'tcx>(
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
        let mut parent_impl = None;
        let (is_trait_impl, is_port_method) = match parent_node {
            rustc_hir::OwnerNode::Item(Item {
                kind: ItemKind::Impl(imp),
                ..
            }) => {
                parent_impl = Some(imp);
                match &imp.of_trait {
                    Some(trait_ref) => (
                        true,
                        trait_ref.trait_ref.trait_def_id().is_some_and(|trait_did| {
                            port_traits::rvs_is_local_world_port_trait(cx, trait_did)
                        }),
                    ),
                    None => (false, false),
                }
            }
            _ => (false, false),
        };
        let name = impl_item.ident.name.as_str();
        let body = cx.tcx.hir_body(*body_id);
        let attrs = cx.tcx.hir_attrs(impl_item.hir_id());
        let is_test = utils::rvs_has_attr(attrs, "test") || test_fn_names.contains(name);
        let is_pub = !is_trait_impl && cx.tcx.visibility(impl_item.owner_id.def_id).is_public();
        let scope = rvs_diagnostic_scope_B(cx, impl_item.owner_id.def_id.to_def_id(), is_test);
        let body_facts =
            body::rvs_collect_body_facts_B(cx, body, data.mode.rvs_should_emit_lints());
        if data.mode.rvs_should_emit_lints()
            && scope == DiagnosticScope::Production
            && !is_trait_impl
            && let Some(imp) = parent_impl
            && let rustc_hir::OwnerNode::Item(parent_item) = parent_node
            && let Some(adt_def_id) = data_struct::rvs_inherent_struct_def_id(cx, parent_item, imp)
            && data_struct::rvs_is_public_fields_data(cx, adt_def_id)
        {
            let struct_name = cx.tcx.item_name(adt_def_id);
            data_struct::rvs_check_data_method_S(
                cx,
                sig,
                body,
                impl_item.owner_id.def_id.to_def_id(),
                struct_name.as_str(),
                name,
                adt_def_id,
            );
        }
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
        rvs_run_body_fn_pipeline_BMS(cx, &subject, data, scope, |scope| {
            ctx::rvs_record_test_site_M(
                is_test,
                name,
                impl_item.hir_id(),
                impl_item.span,
                &body_facts,
                test_names,
                test_calls,
            );
            if scope == DiagnosticScope::Production {
                if !is_test && is_pub {
                    missing_doc::rvs_check_fn_S(cx, name, impl_item.span, attrs, true);
                }
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    impl_item.hir_id(),
                    impl_item.span,
                    &sig.header.safety,
                );
            }
        });
        if data.mode.rvs_should_emit_lints()
            && scope == DiagnosticScope::Production
            && (is_trait_impl && !is_port_method)
        {
            todo_comment::rvs_check_fn_S(cx, impl_item.span);
        }
    }
}

/// Check trait method (provided or required).
fn rvs_check_trait_item_BMS<'tcx>(
    cx: &LateContext<'tcx>,
    trait_item: &'tcx rustc_hir::TraitItem<'tcx>,
    data: &mut FnCheckData<'_>,
) {
    use rustc_hir::{TraitFn, TraitItemKind};

    // Determine if this trait item belongs to a Port trait.
    let parent = cx.tcx.hir_get_parent_item(trait_item.hir_id());
    let parent_def_id = parent.def_id.to_def_id();
    let is_port_trait = port_traits::rvs_is_local_world_port_trait(cx, parent_def_id);
    let is_pub = cx.tcx.visibility(parent_def_id).is_public();
    let attrs = cx.tcx.hir_attrs(trait_item.hir_id());

    match &trait_item.kind {
        TraitItemKind::Fn(sig, TraitFn::Provided(body_id)) => {
            let body = cx.tcx.hir_body(*body_id);
            let body_facts =
                body::rvs_collect_body_facts_B(cx, body, data.mode.rvs_should_emit_lints());
            let scope = rvs_diagnostic_scope_B(cx, trait_item.owner_id.def_id.to_def_id(), false);
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
            rvs_run_body_fn_pipeline_BMS(cx, &subject, data, scope, |scope| {
                if scope == DiagnosticScope::Production {
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
                }
            });
        }
        TraitItemKind::Fn(sig, TraitFn::Required(_)) => {
            if data.mode.rvs_should_emit_lints()
                && rvs_diagnostic_scope_B(cx, trait_item.owner_id.def_id.to_def_id(), false)
                    == DiagnosticScope::Production
            {
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
                // A/C/U are measured from the signature/body, never from the
                // name; required methods still have a signature to read.
                let facts = CapabilityFacts::rvs_from_signature(
                    sig,
                    utils::rvs_has_mutable_params(sig),
                    is_port_trait,
                );
                let mut effective_caps = CapabilityPolicy::rvs_signature_caps(facts);
                if is_port_trait {
                    effective_caps.rvs_insert_M(Capability::P);
                }
                validate::rvs_check_fn_S(cx, name, sig, &effective_caps);
                missing_doc::rvs_check_fn_S(cx, name, trait_item.span, attrs, is_pub);
                missing_safety_doc::rvs_check_fn_S(
                    cx,
                    trait_item.hir_id(),
                    trait_item.span,
                    &sig.header.safety,
                );
            }
            // Required methods (no body) — collect signature info for callgraph.
            if data.mode.rvs_collect_caps_facts() {
                let def_path = callgraph::rvs_collect_callgraph_for_signature_BMS(
                    data.callgraph,
                    cx,
                    trait_item.hir_id(),
                    trait_item.ident,
                    sig,
                    false,
                    is_port_trait,
                    data.crate_provenance,
                );
                data.diagnostic_spans.insert(
                    FunctionIdentity {
                        crate_id: cx
                            .tcx
                            .stable_crate_id(trait_item.hir_id().owner.def_id.to_def_id().krate)
                            .as_u64(),
                        def_path,
                    },
                    (trait_item.hir_id(), trait_item.span),
                );
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
    fn test_20260819_offline_severity_matches_lint_level() {
        // Error-severity offline diagnostics must map to Deny lints and
        // warnings to Warn lints, so enforcement survives the rustc
        // emission path without `-D warnings`.
        let level_for = |lint: crate::offline_caps::OfflineCapsLint| {
            let rustc_lint = rvs_offline_caps_lint_S(lint);
            RIVUS_LINTS
                .iter()
                .zip(RIVUS_LINT_LEVELS.iter())
                .find(|(candidate, _)| std::ptr::eq(**candidate, rustc_lint))
                .map(|(_, level)| *level)
                .expect("never: every offline lint maps to a declared rustc lint")
        };
        use crate::offline_caps::OfflineCapsLint;
        use rustc_lint::Level;
        let mut output = String::new();
        for lint in [
            OfflineCapsLint::ContractMismatch,
            OfflineCapsLint::NonSuffixCapInSuffix,
        ] {
            assert_eq!(level_for(lint), Level::Deny, "{lint:?} must be Deny");
            output.push_str(&format!("{lint:?}=Deny\n"));
        }
        for lint in [
            OfflineCapsLint::MissingRvsPrefix,
            OfflineCapsLint::DuplicateSuffix,
            OfflineCapsLint::IncompleteCapsKnowledge,
            OfflineCapsLint::NonAlphabeticalSuffix,
            OfflineCapsLint::TraitImplOutlier,
            OfflineCapsLint::UnknownCallee,
            OfflineCapsLint::UnknownSuffixLetter,
        ] {
            assert_eq!(level_for(lint), Level::Warn, "{lint:?} must be Warn");
            output.push_str(&format!("{lint:?}=Warn\n"));
        }
        rvs_snapshot_BIS("test_20260819_offline_severity_matches_lint_level", &output);
    }

    #[test]
    fn test_20260716_missing_call_site_does_not_fall_back_to_function_span() {
        let identity = FunctionIdentity {
            crate_id: 7,
            def_path: crate::symbols::DefPath::from("demo::rvs_call"),
        };
        let call_site = CallSiteIdentity {
            callee: FunctionIdentity {
                crate_id: 9,
                def_path: crate::symbols::DefPath::from("dependency::effect"),
            },
            occurrence: 0,
            source: None,
        };
        let anchor = crate::offline_caps::OfflineCapsEmissionAnchor {
            identity: identity.clone(),
            call_site: Some(call_site),
        };
        let function_locations = BTreeMap::from([(identity, "function")]);
        let call_locations = BTreeMap::new();

        let location = rvs_resolve_emission_location(&anchor, &function_locations, &call_locations);
        let output = format!("location={location:?}\n");
        rvs_snapshot_BIS(
            "test_20260716_missing_call_site_does_not_fall_back_to_function_span",
            &output,
        );

        assert_eq!(location, None);
    }
}
