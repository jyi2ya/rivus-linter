use std::collections::BTreeMap;
use std::path::PathBuf;

use rustc_hir::HirId;
use rustc_lint::LateContext;
use rustc_span::{FileName, Ident, Span};

use super::super::ctx::FnSubject;
use super::super::utils::{
    CallTarget, rvs_count_body_effective_lines, rvs_def_path, rvs_has_allow, rvs_has_mutable_params,
};
use crate::artifacts::{
    CallEdgeType, CallSiteIdentity, CallSiteSource, CrateProvenance, FnGraph, FnNode, FnSource,
    FunctionIdentity,
};
use crate::capability::{CapabilityFacts, ParsedFunctionName};
use crate::symbols::DefPath;

#[derive(Debug)]
pub(crate) struct CollectedCallSite {
    pub(crate) identity: CallSiteIdentity,
    pub(crate) hir_id: HirId,
    pub(crate) span: Span,
}

#[derive(Debug)]
pub(crate) struct CollectedCallgraphItem {
    pub(crate) caller: FunctionIdentity,
    pub(crate) call_sites: Vec<CollectedCallSite>,
}

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

pub(crate) fn rvs_collect_callgraph_for_item_BMS<'tcx>(
    callgraph: &mut FnGraph,
    cx: &LateContext<'tcx>,
    subject: &FnSubject<'_, 'tcx>,
    crate_provenance: CrateProvenance,
) -> CollectedCallgraphItem {
    let local_def_id = subject.hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let cargo_package_name = crate::symbols::rvs_cargo_package_name_BS();
    if caller_path.rvs_is_build_script_for_package(cargo_package_name.as_deref()) {
        return CollectedCallgraphItem {
            caller: FunctionIdentity {
                crate_id: cx.tcx.stable_crate_id(def_id.krate).as_u64(),
                def_path: caller_path,
            },
            call_sites: Vec::new(),
        };
    }
    let is_in_test_module = caller_path.rvs_is_in_test_module();
    let is_entrypoint = rvs_is_executable_entry(cx, def_id);
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
        is_in_test_module,
        subject.span,
    );

    let resolved_edges: Vec<(
        DefPath,
        FunctionIdentity,
        Option<CallSiteSource>,
        crate::lints::utils::ObservationKind,
        HirId,
        Span,
    )> = subject
        .body_facts
        .call_observations
        .iter()
        .filter(|observation| {
            observation.kind != crate::lints::utils::ObservationKind::UnsupportedIndirect
        })
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
                    observation.kind,
                    observation.hir_id,
                    observation.span,
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
        if is_reportable && !subject.is_test && !is_in_test_module && !allows_dead_code {
            Some(rvs_count_body_effective_lines(cx, subject.body))
        } else {
            None
        };

    let mut target_calls: BTreeMap<FunctionIdentity, CallEdgeType> = BTreeMap::new();
    let mut all_call_sites: Vec<CollectedCallSite> = Vec::new();
    for (edge_index, edge) in resolved_edges.iter().enumerate() {
        let (_, identity, source, kind, hir_id, span) = edge;
        let edge_type = match kind {
            crate::lints::utils::ObservationKind::Direct => CallEdgeType::Strong,
            crate::lints::utils::ObservationKind::FunctionReference => CallEdgeType::Weak,
            crate::lints::utils::ObservationKind::UnsupportedIndirect => {
                debug_assert!(
                    false,
                    "unsupported indirect observations are filtered before edge projection"
                );
                continue;
            }
        };
        match target_calls.get(identity) {
            Some(CallEdgeType::Strong) => {}
            _ => {
                target_calls.insert(identity.clone(), edge_type);
            }
        }
        let occurrence = u32::try_from(edge_index)
            .expect("never: a function cannot contain more than u32::MAX resolved calls");
        all_call_sites.push(CollectedCallSite {
            identity: CallSiteIdentity {
                callee: identity.clone(),
                occurrence,
                source: source.clone(),
            },
            hir_id: *hir_id,
            span: *span,
        });
    }
    node.calls = target_calls.clone();
    node.call_sites = all_call_sites
        .iter()
        .map(|call_site| call_site.identity.clone())
        .collect();
    if subject.is_test {
        node.unresolved_test_calls = subject
            .body_facts
            .call_observations
            .iter()
            .filter(|observation| observation.kind == crate::lints::utils::ObservationKind::Direct)
            .filter_map(|observation| match &observation.target {
                CallTarget::UnresolvedPath { path } => {
                    Some(crate::lints::body::collector::rvs_unresolved_call_name(path).to_string())
                }
                CallTarget::UnresolvedMethod { name } => Some(name.clone()),
                CallTarget::Resolved { .. } => None,
            })
            .filter(|name| name.starts_with("rvs_"))
            .collect();
    }
    let is_coverage_candidate = !is_in_test_module
        && !is_entrypoint
        && !subject.is_test
        && !subject.is_trait_impl
        && !allows_dead_code;
    node.has_body = true;
    node.report_line_count = report_line_count;
    node.report_function_count = usize::from(report_line_count.is_some());
    node.allows_dead_code = allows_dead_code;
    node.is_production = !is_in_test_module;
    node.is_coverage_candidate = is_coverage_candidate;
    node.crate_provenance = crate_provenance;
    node.crate_id = crate_id;
    if let Err(error) = callgraph.rvs_merge_node_M(&caller_path, &node) {
        cx.tcx
            .dcx()
            .err(format!("cannot merge collected callgraph node: {error}"));
    }
    CollectedCallgraphItem {
        caller: FunctionIdentity {
            crate_id,
            def_path: caller_path,
        },
        call_sites: all_call_sites,
    }
}

/// Collect callgraph entry from a signature alone (no body — e.g. trait method
/// declarations without default implementation).
pub(crate) fn rvs_collect_callgraph_for_signature_BMS(
    callgraph: &mut FnGraph,
    cx: &LateContext<'_>,
    hir_id: HirId,
    ident: Ident,
    sig: &rustc_hir::FnSig<'_>,
    is_trait_impl: bool,
    is_port_method: bool,
    crate_provenance: CrateProvenance,
) -> DefPath {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let cargo_package_name = crate::symbols::rvs_cargo_package_name_BS();
    if caller_path.rvs_is_build_script_for_package(cargo_package_name.as_deref()) {
        return caller_path;
    }
    let is_in_test_module = caller_path.rvs_is_in_test_module();
    let crate_id = cx.tcx.stable_crate_id(def_id.krate).as_u64();
    let mut node = rvs_fn_node_from_signature(
        cx,
        ident,
        sig,
        is_trait_impl,
        false,
        is_port_method,
        false,
        is_in_test_module,
        cx.tcx.hir_span(hir_id),
    );
    node.has_body = false;
    node.is_production = !is_in_test_module;
    node.crate_provenance = crate_provenance;
    node.crate_id = crate_id;
    if let Err(error) = callgraph.rvs_merge_node_M(&caller_path, &node) {
        cx.tcx
            .dcx()
            .err(format!("cannot merge collected callgraph node: {error}"));
    }
    caller_path
}

fn rvs_is_executable_entry(cx: &LateContext<'_>, def_id: rustc_span::def_id::DefId) -> bool {
    if cx
        .tcx
        .entry_fn(())
        .is_some_and(|(entry_def_id, _)| entry_def_id == def_id)
    {
        return true;
    }
    let path = rvs_def_path(cx, def_id);
    let segs: Vec<&str> = path.split("::").collect();
    segs.get(1)
        .is_some_and(|seg| segs.len() == 2 && *seg == "main")
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
    use crate::test_support::rvs_register_test_coverage;

    #[test]
    fn test_20260630_callgraph_helper_coverage() {
        rvs_register_test_coverage((
            rvs_collect_callgraph_for_item_BMS,
            rvs_collect_callgraph_for_signature_BMS,
            rvs_fn_source,
            rvs_real_file_name,
        ));
    }
}
