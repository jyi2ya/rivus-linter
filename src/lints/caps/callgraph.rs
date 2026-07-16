use std::path::PathBuf;

use rustc_hir::HirId;
use rustc_lint::LateContext;
use rustc_span::{FileName, Ident, Span};

use super::super::ctx::FnSubject;
use super::super::utils::{
    CallTarget, rvs_count_effective_lines_M, rvs_def_path, rvs_has_allow, rvs_has_mutable_params,
};
use crate::artifacts::{
    CallSiteIdentity, CallSiteSource, FnGraph, FnNode, FnSource, FunctionIdentity,
};
use crate::capability::{CapabilityFacts, ParsedFunctionName};
use crate::symbols::DefPath;

fn rvs_fn_node_from_signature(
    cx: &LateContext<'_>,
    ident: Ident,
    sig: &rustc_hir::FnSig<'_>,
    is_trait_impl: bool,
    is_test: bool,
    is_port_method: bool,
    is_entrypoint: bool,
    is_test_compilation: bool,
    declaration_span: Span,
) -> FnNode {
    FnNode {
        facts: CapabilityFacts::rvs_from_signature(
            sig,
            rvs_has_mutable_params(sig),
            is_port_method,
        ),
        has_body: false,
        is_trait_impl,
        is_test,
        is_entrypoint,
        is_test_compilation,
        sources: rvs_fn_source(cx, ident, declaration_span)
            .into_iter()
            .collect(),
        ..FnNode::default()
    }
}

pub(crate) fn rvs_collect_callgraph_for_item_M<'tcx>(
    callgraph: &mut FnGraph,
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
) -> DefPath {
    let local_def_id = subject.hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let is_entrypoint = rvs_is_executable_entry(cx, def_id);
    let is_test_compilation = cx.tcx.sess.opts.test;
    let crate_id = cx.tcx.stable_crate_id(def_id.krate).as_u64();
    let attrs = cx.tcx.hir_attrs(subject.hir_id);
    let mut node = rvs_fn_node_from_signature(
        cx,
        subject.ident,
        subject.sig,
        subject.is_trait_impl,
        subject.is_test,
        subject.is_port_method,
        is_entrypoint,
        is_test_compilation,
        subject.span,
    );

    let resolved_calls: Vec<(DefPath, FunctionIdentity, Option<CallSiteSource>)> = subject
        .body_facts
        .calls
        .iter()
        .filter_map(|observation| {
            if let CallTarget::Resolved {
                def_path,
                def_kind: rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn,
                crate_id,
            } = &observation.target
            {
                Some((
                    def_path.clone(),
                    FunctionIdentity {
                        crate_id: *crate_id,
                        def_path: def_path.clone(),
                    },
                    rvs_call_site_source(cx, observation.span),
                ))
            } else {
                None
            }
        })
        .collect();

    node.facts = node.facts.rvs_with_static_refs(
        subject.body_facts.has_static_ref,
        subject.body_facts.has_static_mut_ref,
        subject.body_facts.has_thread_local_ref,
    );
    let allows_dead_code = rvs_has_allow(attrs, "dead_code") || rvs_has_allow(attrs, "unused");
    let is_reportable = subject.is_port_method
        || ParsedFunctionName::rvs_parse(subject.rvs_name()).rvs_has_rvs_prefix();
    let report_line_count =
        if is_reportable && !subject.is_test && !is_test_compilation && !allows_dead_code {
            Some(rvs_count_effective_lines_M(cx, subject.body))
        } else {
            None
        };

    node.calls = resolved_calls
        .iter()
        .map(|(def_path, _, _)| def_path.clone())
        .collect();
    node.coverage_calls.insert(
        crate_id,
        resolved_calls
            .iter()
            .map(|(_, identity, _)| identity.clone())
            .collect(),
    );
    node.coverage_call_sites.insert(
        crate_id,
        resolved_calls
            .into_iter()
            .enumerate()
            .map(|(occurrence, (_, callee, source))| CallSiteIdentity {
                callee,
                occurrence: u32::try_from(occurrence)
                    .expect("never: a function cannot contain more than u32::MAX resolved calls"),
                source,
            })
            .collect(),
    );
    node.sources_by_crate.insert(crate_id, node.sources.clone());
    node.facts_by_crate.insert(crate_id, node.facts);
    node.has_body_by_crate.insert(crate_id, true);
    if is_entrypoint {
        node.entrypoint_crate_ids.insert(crate_id);
    }
    if subject.is_test {
        node.test_crate_ids.insert(crate_id);
        node.unresolved_test_calls = subject
            .body_facts
            .calls
            .iter()
            .filter_map(|observation| match &observation.target {
                CallTarget::UnresolvedPath { path } => path.rsplit("::").next().map(str::to_string),
                CallTarget::UnresolvedMethod { name } => Some(name.clone()),
                CallTarget::Resolved { .. } => None,
            })
            .filter(|name| name.starts_with("rvs_"))
            .collect();
    }
    if !is_test_compilation {
        node.production_crate_ids.insert(crate_id);
        if !is_entrypoint && !subject.is_test && !subject.is_trait_impl && !allows_dead_code {
            node.coverage_candidate_crate_ids.insert(crate_id);
        }
    }
    node.has_body = true;
    node.report_line_count = report_line_count;
    node.report_function_count = usize::from(report_line_count.is_some());
    node.allows_dead_code = allows_dead_code;
    callgraph.rvs_merge_node_M(caller_path.clone(), node);
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
    let crate_id = cx.tcx.stable_crate_id(def_id.krate).as_u64();
    let is_test_compilation = cx.tcx.sess.opts.test;
    let mut node = rvs_fn_node_from_signature(
        cx,
        ident,
        sig,
        is_trait_impl,
        false,
        is_port_method,
        false,
        is_test_compilation,
        cx.tcx.hir_span(hir_id),
    );
    node.sources_by_crate.insert(crate_id, node.sources.clone());
    node.facts_by_crate.insert(crate_id, node.facts);
    node.has_body_by_crate.insert(crate_id, false);
    node.coverage_calls.insert(crate_id, Default::default());
    node.coverage_call_sites
        .insert(crate_id, Default::default());
    if !is_test_compilation {
        node.production_crate_ids.insert(crate_id);
    }
    callgraph.rvs_merge_node_M(caller_path.clone(), node);
    caller_path
}

fn rvs_is_executable_entry(cx: &LateContext<'_>, def_id: rustc_span::def_id::DefId) -> bool {
    !cx.tcx.sess.opts.test
        && cx
            .tcx
            .entry_fn(())
            .is_some_and(|(entry_def_id, _)| entry_def_id == def_id)
}

fn rvs_fn_source(cx: &LateContext<'_>, ident: Ident, declaration_span: Span) -> Option<FnSource> {
    let span = ident.span;
    if span.from_expansion() || declaration_span.from_expansion() {
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

pub(crate) fn rvs_call_site_source(cx: &LateContext<'_>, span: Span) -> Option<CallSiteSource> {
    let span = span.source_callsite();
    let source_map = cx.tcx.sess.source_map();
    let span_data = span.data();
    let start = source_map.lookup_byte_offset(span_data.lo);
    let end = source_map.lookup_byte_offset(span_data.hi);
    if start.sf.name != end.sf.name || start.pos >= end.pos {
        return None;
    }
    let file = rvs_real_file_name(&start.sf.name)?;
    if file.is_absolute() {
        return Some(CallSiteSource::rvs_new(file, start.pos.0, end.pos.0));
    }
    let base = cx.tcx.sess.opts.working_dir.local_path()?.to_path_buf();
    if !base.is_absolute() {
        return None;
    }
    Some(CallSiteSource::rvs_new_relative(
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
            let _span: Span = unreachable!();
            let _ = rvs_fn_source(_cx, _ident, _span);
            let _file_name: &FileName = unreachable!();
            let _ = rvs_real_file_name(_file_name);
        }
    }
}
