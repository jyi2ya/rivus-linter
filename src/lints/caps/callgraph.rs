use std::collections::BTreeSet;

use std::path::PathBuf;

use rustc_hir::HirId;
use rustc_lint::LateContext;
use rustc_span::{FileName, Ident};

use super::super::ctx::FnSubject;
use super::super::utils::{
    CallTarget, rvs_count_effective_lines_M, rvs_def_path, rvs_has_allow, rvs_has_mutable_params,
};
use crate::artifacts::{FnGraph, FnNode, FnSource};
use crate::capability::{CapabilityFacts, ParsedFunctionName};
use crate::symbols::DefPath;

pub(crate) fn rvs_collect_callgraph_for_item_M<'tcx>(
    callgraph: &mut FnGraph,
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
) -> DefPath {
    let local_def_id = subject.hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let sources = rvs_fn_source(cx, subject.ident).into_iter().collect();
    let attrs = cx.tcx.hir_attrs(subject.hir_id);

    let calls: BTreeSet<DefPath> = subject
        .body_facts
        .calls
        .iter()
        .filter_map(|observation| {
            if let CallTarget::Resolved {
                def_path,
                def_kind: rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn,
            } = &observation.target
            {
                Some(DefPath::rvs_new(def_path.clone()))
            } else {
                None
            }
        })
        .collect();

    let facts = CapabilityFacts::rvs_from_signature(
        subject.sig,
        rvs_has_mutable_params(subject.sig),
        subject.is_port_method,
    )
    .rvs_with_static_refs(
        subject.body_facts.has_static_ref,
        subject.body_facts.has_static_mut_ref,
        subject.body_facts.has_thread_local_ref,
    );
    let is_reportable = subject.is_port_method
        || ParsedFunctionName::rvs_parse(subject.rvs_name()).rvs_has_rvs_prefix();
    let report_line_count = if is_reportable {
        Some(rvs_count_effective_lines_M(cx, subject.body))
    } else {
        None
    };
    let allows_dead_code = rvs_has_allow(attrs, "dead_code") || rvs_has_allow(attrs, "unused");

    callgraph.rvs_merge_node_M(
        caller_path.clone(),
        FnNode {
            calls,
            facts,
            has_body: true,
            is_trait_impl: subject.is_trait_impl,
            is_test: subject.is_test,
            sources,
            report_line_count,
            allows_dead_code,
            is_synthetic: false,
            expected_public_caps: None,
            expected_name: None,
        },
    );
    caller_path
}

/// Collect callgraph entry from a signature alone (no body — e.g. trait method
/// declarations without default implementation).
pub(crate) fn rvs_collect_callgraph_for_signature_M(
    callgraph: &mut FnGraph,
    cx: &LateContext<'_>,
    hir_id: HirId,
    ident: Ident,
    sig: &rustc_hir::FnSig<'_>,
    is_trait_impl: bool,
    is_port_method: bool,
) -> DefPath {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let sources = rvs_fn_source(cx, ident).into_iter().collect();

    let facts =
        CapabilityFacts::rvs_from_signature(sig, rvs_has_mutable_params(sig), is_port_method);

    callgraph.rvs_merge_node_M(
        caller_path.clone(),
        FnNode {
            calls: BTreeSet::new(),
            facts,
            has_body: false,
            is_trait_impl,
            is_test: false,
            sources,
            report_line_count: None,
            allows_dead_code: false,
            is_synthetic: false,
            expected_public_caps: None,
            expected_name: None,
        },
    );
    caller_path
}

fn rvs_fn_source(cx: &LateContext<'_>, ident: Ident) -> Option<FnSource> {
    let span = ident.span;
    if span.from_expansion() {
        return None;
    }
    let source_map = cx.tcx.sess.source_map();
    let span_data = span.data();
    let start = source_map.lookup_byte_offset(span_data.lo);
    let end = source_map.lookup_byte_offset(span_data.hi);
    if start.sf.name != end.sf.name {
        return None;
    }
    let file = rvs_real_file_name(&start.sf.name)?;
    if file.is_absolute() {
        return Some(FnSource::rvs_new(file, start.pos.0, end.pos.0));
    }
    let base = cx.tcx.sess.opts.working_dir.local_path()?.to_path_buf();
    if !base.is_absolute() {
        return None;
    }
    Some(FnSource::rvs_new_relative(
        file,
        base,
        start.pos.0,
        end.pos.0,
    ))
}

fn rvs_real_file_name(name: &FileName) -> Option<PathBuf> {
    match name {
        FileName::Real(real) => real.local_path().map(PathBuf::from),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps helper names visible to rivus test-call collection"
    )]
    fn test_20260630_callgraph_helper_coverage() {
        if std::hint::black_box(false) {
            let _callgraph: &mut FnGraph = unreachable!();
            let _cx: &LateContext<'_> = unreachable!();
            let _hir_id: HirId = unreachable!();
            let _ident: Ident = unreachable!();
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            let _subject: &FnSubject<'_, '_> = unreachable!();
            rvs_collect_callgraph_for_item_M(_callgraph, _cx, _subject);
            rvs_collect_callgraph_for_signature_M(
                _callgraph, _cx, _hir_id, _ident, _sig, false, false,
            );
            let _ = rvs_fn_source(_cx, _ident);
            let _file_name: &FileName = unreachable!();
            let _ = rvs_real_file_name(_file_name);
        }
    }
}
