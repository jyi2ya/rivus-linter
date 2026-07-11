use std::collections::{BTreeMap, HashSet};

use rustc_hir::HirId;
use rustc_span::{Ident, Span};

use crate::artifacts::FnGraph;
use crate::symbols::DefPath;
use rustc_span::def_id::DefId;

use super::body::BodyFacts;

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
    pub good_fns: &'a mut Vec<(String, Span)>,
    pub ok_fns: &'a mut Vec<(String, Span)>,
    pub callgraph: &'a mut FnGraph,
    pub diagnostic_spans: &'a mut BTreeMap<DefPath, (HirId, Span)>,
    pub collect_caps_facts: bool,
    pub should_emit_lints: bool,
    pub port_traits: &'a HashSet<DefId>,
}
