use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::marker::PhantomData;

use crate::artifacts::{CrateProvenance, FnGraph, FunctionIdentity};
use crate::capsmap::CapsMap;
use crate::offline_caps::OfflineCapsEmission;
use crate::symbols::CrateName;

pub(crate) trait LintEnvironment {
    type World: Debug;

    fn rvs_write_callgraph_BIMPST(
        world: &mut Self::World,
        crate_name: &CrateName,
        callgraph: &FnGraph,
    ) -> Result<(), String>;

    fn rvs_acknowledge_offline_emission_BIMPS(
        world: &mut Self::World,
        emission_index: usize,
        anchor_index: usize,
    ) -> Result<(), String>;
}

#[derive(Debug)]
pub(crate) struct RivusLintConfig<E: LintEnvironment> {
    pub(crate) capsmap: Result<Option<CapsMap>, String>,
    pub(crate) untested_functions:
        Result<Option<BTreeMap<FunctionIdentity, crate::artifacts::CoverageLabel>>, String>,
    pub(crate) offline_emissions: Result<Vec<OfflineCapsEmission>, String>,
    pub(crate) test_outputs: Option<BTreeSet<String>>,
    pub(crate) collect_callgraph: bool,
    pub(crate) should_emit_caps_report: bool,
    pub(crate) ui_testing: bool,
    pub(crate) crate_provenance: CrateProvenance,
    pub(crate) world: E::World,
    pub(crate) interpreter: PhantomData<E>,
}
