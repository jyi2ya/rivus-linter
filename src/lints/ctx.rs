use std::collections::{BTreeMap, HashSet};

use rustc_span::Span;

use super::callgraph::{FnBehavior, FnReportEntry};
use crate::capsmap::CapsMap;
use rustc_span::def_id::DefId;

/// Bundles the mutable references needed by fn-level checks so they can be
/// threaded through without leaking RivusLintPass internals.
pub(crate) struct FnCheckData<'a> {
    pub capsmap: &'a Option<CapsMap>,
    pub good_fns: &'a mut Vec<(String, Span)>,
    pub ok_fns: &'a mut Vec<(String, Span)>,
    pub fn_report: &'a mut Vec<FnReportEntry>,
    pub callgraph: &'a mut BTreeMap<String, FnBehavior>,
    pub collect_callgraph: bool,
    pub should_emit_lints: bool,
    pub port_traits: &'a HashSet<DefId>,
}
