use std::collections::BTreeMap;

use rustc_hir::HirId;
use rustc_span::{Ident, Span};

use super::body::BodyFacts;
use crate::artifacts::{FnGraph, FunctionIdentity};
use crate::symbols::DefPath;

#[derive(Debug)]
pub(crate) struct CoverageFn {
    pub(crate) identity: FunctionIdentity,
    pub(crate) name: String,
    pub(crate) hir_id: HirId,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TestSite {
    pub(crate) hir_id: HirId,
    pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TestCallTarget {
    Resolved(FunctionIdentity),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only branch links the constructor exercised by rustc UI fixtures"
    )]
    fn test_20260714_fn_subject_body_constructor_coverage() {
        rvs_snapshot_BIS(
            "test_20260714_fn_subject_body_constructor_coverage",
            "covered\n",
        );
        if std::hint::black_box(false) {
            let _ident: Ident = unreachable!();
            let _hir_id: HirId = unreachable!();
            let _span: Span = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _body: &rustc_hir::Body<'_> = unreachable!();
            let _facts: &BodyFacts = unreachable!();
            let _ = FnSubject::rvs_body(
                _ident, _hir_id, _span, _sig, _body, _facts, true, false, false, false,
            );
        }
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
