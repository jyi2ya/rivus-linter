use std::collections::BTreeMap;

use rustc_hir::HirId;
use rustc_span::{Ident, Span};

use super::body::BodyFacts;
use crate::artifacts::FnGraph;
use crate::symbols::DefPath;

#[derive(Debug)]
pub(crate) struct CoverageFn {
    pub(crate) def_path: DefPath,
    pub(crate) name: String,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TestCallTarget {
    Resolved(DefPath),
    UnresolvedName(String),
}

#[derive(Debug)]
pub(crate) struct FnSubject<'facts, 'tcx> {
    pub(crate) ident: Ident,
    pub(crate) hir_id: HirId,
    pub(crate) span: Span,
    pub(crate) sig: &'tcx rustc_hir::FnSig<'tcx>,
    pub(crate) body: &'tcx rustc_hir::Body<'tcx>,
    pub(crate) body_facts: &'facts BodyFacts,
    pub(crate) has_body: bool,
    pub(crate) is_test: bool,
    pub(crate) is_trait_impl: bool,
    pub(crate) is_port_method: bool,
}

impl<'facts, 'tcx> FnSubject<'facts, 'tcx> {
    pub(crate) fn rvs_body(
        ident: Ident,
        hir_id: HirId,
        span: Span,
        sig: &'tcx rustc_hir::FnSig<'tcx>,
        body: &'tcx rustc_hir::Body<'tcx>,
        body_facts: &'facts BodyFacts,
        has_body: bool,
        is_test: bool,
        is_trait_impl: bool,
        is_port_method: bool,
    ) -> Self {
        Self {
            ident,
            hir_id,
            span,
            sig,
            body,
            body_facts,
            has_body,
            is_test,
            is_trait_impl,
            is_port_method,
        }
    }

    pub(crate) fn rvs_name(&self) -> &str {
        self.ident.name.as_str()
    }
}

/// Bundles the mutable references needed by fn-level checks so they can be
/// threaded through without leaking RivusLintPass internals.
#[derive(Debug)]
pub(crate) struct FnCheckData<'a> {
    pub good_fns: &'a mut Vec<CoverageFn>,
    pub ok_fns: &'a mut Vec<CoverageFn>,
    pub callgraph: &'a mut FnGraph,
    pub diagnostic_spans: &'a mut BTreeMap<DefPath, (HirId, Span)>,
    pub collect_caps_facts: bool,
    pub should_emit_lints: bool,
}
