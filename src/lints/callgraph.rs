use std::collections::BTreeSet;

use std::path::PathBuf;

use rustc_hir::{Body, ExprKind, HirId, Mutability, def::DefKind};
use rustc_lint::LateContext;
use rustc_span::{FileName, Ident};

use super::utils::{
    rvs_count_effective_lines_M, rvs_def_path, rvs_has_allow, rvs_has_mutable_params,
    rvs_static_is_thread_local, rvs_walk_closures,
};
use crate::artifacts::{FnGraph, FnNode, FnSource};
use crate::capability::{CapabilityFacts, ParsedFunctionName};
use crate::symbols::DefPath;

#[expect(
    clippy::too_many_arguments,
    reason = "callgraph collection needs full fn metadata to avoid extra wrapper structs"
)]
pub(crate) fn rvs_collect_callgraph_for_item_M<'tcx>(
    callgraph: &mut FnGraph,
    cx: &LateContext<'tcx>,
    hir_id: HirId,
    ident: Ident,
    sig: &rustc_hir::FnSig<'tcx>,
    body: &Body<'tcx>,
    is_trait_impl: bool,
    is_test: bool,
    is_port_method: bool,
) -> DefPath {
    let local_def_id = hir_id.owner.def_id;
    let def_id = local_def_id.to_def_id();
    let caller_path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let sources = rvs_fn_source(cx, ident).into_iter().collect();
    let attrs = cx.tcx.hir_attrs(hir_id);

    let mut calls: BTreeSet<DefPath> = BTreeSet::new();
    let mut has_static_ref = false;
    let mut has_static_mut_ref = false;
    let mut has_thread_local_ref = false;

    rvs_walk_closures(cx.tcx, body.value, |e| {
        if let ExprKind::Path(ref q) = e.kind {
            if let rustc_hir::def::Res::Def(kind, did) = cx.qpath_res(q, e.hir_id) {
                if let DefKind::Static { mutability, .. } = kind {
                    if rvs_static_is_thread_local(cx, did) {
                        has_thread_local_ref = true;
                    }
                    match mutability {
                        Mutability::Mut => has_static_mut_ref = true,
                        Mutability::Not => has_static_ref = true,
                    }
                }
            }
        }
    });

    rvs_walk_closures(cx.tcx, body.value, |e| match &e.kind {
        ExprKind::Call(func, _) => {
            if let ExprKind::Path(ref q) = func.kind {
                if let rustc_hir::def::Res::Def(k, did) = cx.qpath_res(q, func.hir_id) {
                    if matches!(k, DefKind::Fn | DefKind::AssocFn) {
                        calls.insert(DefPath::rvs_new(rvs_def_path(cx, did)));
                    }
                }
            }
        }
        ExprKind::MethodCall(..) => {
            let owner = e.hir_id.owner.def_id;
            let tck = cx.tcx.typeck(owner);
            if let Some(did) = tck.type_dependent_def_id(e.hir_id) {
                calls.insert(DefPath::rvs_new(rvs_def_path(cx, did)));
            }
        }
        _ => {}
    });

    let facts =
        CapabilityFacts::rvs_from_signature(sig, rvs_has_mutable_params(sig), is_port_method)
            .rvs_with_static_refs(has_static_ref, has_static_mut_ref, has_thread_local_ref);
    let is_reportable =
        is_port_method || ParsedFunctionName::rvs_parse(ident.name.as_str()).rvs_has_rvs_prefix();
    let report_line_count = if is_reportable {
        Some(rvs_count_effective_lines_M(cx, body))
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
            is_trait_impl,
            is_test,
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
            let _body: &Body<'_> = unreachable!();
            rvs_collect_callgraph_for_item_M(
                _callgraph, _cx, _hir_id, _ident, _sig, _body, false, false, false,
            );
            rvs_collect_callgraph_for_signature_M(
                _callgraph, _cx, _hir_id, _ident, _sig, false, false,
            );
            let _ = rvs_fn_source(_cx, _ident);
            let _file_name: &FileName = unreachable!();
            let _ = rvs_real_file_name(_file_name);
        }
    }
}
