use std::collections::{BTreeMap, HashSet};

use rustc_hir::HirId;
use rustc_span::Span;

use crate::artifacts::FnGraph;
use crate::symbols::DefPath;
use rustc_span::def_id::DefId;

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
