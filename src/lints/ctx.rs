use std::collections::BTreeMap;

use rustc_hir::HirId;
use rustc_span::{Ident, Span};

use super::body::BodyFacts;
use crate::artifacts::{CallSiteIdentity, CrateProvenance, FnGraph, FunctionIdentity};

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

/// Shared test-site accounting for free-fn and impl-item handlers:
/// registers the site by name and harvests direct `rvs_` calls from the body.
pub(crate) fn rvs_record_test_site_M(
    is_test: bool,
    name: &str,
    hir_id: HirId,
    span: Span,
    body_facts: &BodyFacts,
    test_names: &mut BTreeMap<String, Vec<TestSite>>,
    test_calls: &mut std::collections::HashSet<TestCallTarget>,
) {
    if !is_test {
        return;
    }
    test_names
        .entry(name.to_string())
        .or_default()
        .push(TestSite { hir_id, span });
    super::body::collector::rvs_collect_test_calls_M(body_facts, test_calls);
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
    pub(crate) const fn rvs_body(
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
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};

    #[test]
    fn test_20260714_fn_subject_body_constructor_coverage() {
        rvs_snapshot_BIS(
            "test_20260714_fn_subject_body_constructor_coverage",
            "covered\n",
        );
        rvs_register_test_coverage(FnSubject::rvs_body);
    }
}

/// Bundles the mutable references needed by fn-level checks so they can be
/// threaded through without leaking RivusLintPass internals.
#[derive(Debug)]
pub(crate) struct FnCheckData<'a> {
    pub good_fns: &'a mut Vec<CoverageFn>,
    pub ok_fns: &'a mut Vec<CoverageFn>,
    pub callgraph: &'a mut FnGraph,
    pub diagnostic_spans: &'a mut BTreeMap<FunctionIdentity, (HirId, Span)>,
    pub diagnostic_call_spans:
        &'a mut BTreeMap<(FunctionIdentity, CallSiteIdentity), (HirId, Span)>,
    pub mode: super::LintExecutionMode,
    pub crate_provenance: CrateProvenance,
}
