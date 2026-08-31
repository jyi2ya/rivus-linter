use std::collections::BTreeSet;
use std::fmt::Debug;
use std::marker::PhantomData;

use crate::artifacts::{CrateProvenance, FnGraph};
use crate::capsmap::CapsMap;
use crate::offline_caps::OfflineCapsEmission;
use crate::symbols::CrateName;

pub(crate) trait LintEnvironment {
    type World: Debug;

    fn rvs_write_callgraph_P(
        world: &mut Self::World,
        crate_name: &CrateName,
        callgraph: &FnGraph,
    ) -> Result<(), String>;

    fn rvs_acknowledge_offline_emission_P(
        world: &mut Self::World,
        emission_index: usize,
        anchor_index: usize,
    ) -> Result<(), String>;
}

/// What one rustc lint-pass process is responsible for. The driver
/// configuration selects the mode once; consumers ask the mode instead of
/// combining boolean execution flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LintExecutionMode {
    /// Collect the function graph and write artifacts; emit no lints.
    CollectOnly,
    /// Collect the function graph, write artifacts, and run the direct
    /// lints in the same process. This is the lint-bearing collection
    /// compile of `cargo rivus check`; its non-zero exit short-circuits
    /// the whole command before graph analysis.
    CheckAndCollect,
    /// Replay merged-graph diagnostics and the untested selection onto HIR
    /// anchors. Direct lints are owned by the collection compile and do
    /// not fire here.
    ReplayDiagnostics,
    /// Direct single-crate analysis: run direct lints and the local caps
    /// report against the in-crate graph.
    ProjectCapsCompatibility,
}

impl LintExecutionMode {
    /// Direct node/body lints fire in this process.
    pub(crate) const fn rvs_should_emit_lints(self) -> bool {
        matches!(self, Self::CheckAndCollect | Self::ProjectCapsCompatibility)
    }

    /// Function-graph facts and diagnostic anchors are collected in this
    /// process. Every current mode collects; the anchor-only replay split
    /// arrives with the check2 elimination migration.
    pub(crate) const fn rvs_collect_caps_facts(self) -> bool {
        true
    }

    /// The single-crate caps report runs in this process.
    pub(crate) const fn rvs_is_caps_report(self) -> bool {
        matches!(self, Self::ProjectCapsCompatibility)
    }
}

#[derive(Debug)]
pub(crate) struct RivusLintConfig<E: LintEnvironment> {
    pub(crate) mode: LintExecutionMode,
    pub(crate) capsmap: Result<Option<CapsMap>, String>,
    pub(crate) offline_emissions: Result<Vec<OfflineCapsEmission>, String>,
    pub(crate) test_outputs: Option<BTreeSet<String>>,
    pub(crate) ui_testing: bool,
    pub(crate) crate_provenance: CrateProvenance,
    pub(crate) world: E::World,
    pub(crate) interpreter: PhantomData<E>,
}
