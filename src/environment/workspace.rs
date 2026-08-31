use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use super::callgraph_cache::{
    rvs_load_published_std_callgraph_cache_BIS, rvs_merge_callgraph_dir_BIS,
    rvs_merge_generation_callgraph_dir_BIS,
};
use super::cargo_targets::{CargoTargetScope, rvs_detect_local_crate_prefixes_BIS};
#[cfg(test)]
use super::cargo_targets::{
    rvs_collect_auto_target_prefixes_BIMS, rvs_collect_local_crate_prefixes,
    rvs_collect_local_crate_prefixes_for_targets, rvs_insert_manifest_crate_name_M,
};
use crate::artifacts::{CrateProvenance, FnGraph};
#[cfg(test)]
use crate::callgraph::rvs_merge_std_like_callgraph_M;
use crate::callgraph::{
    rvs_filter_std_like_callgraph_M, rvs_is_std_like_def_path,
    rvs_merge_std_like_callgraph_with_local_prefixes_M,
};
use crate::capsmap::{self, CapsMap};
use crate::function_classification::LocalScope;
use crate::symbols::CrateName;

const RVS_RUN_GENERATION_MARKER_FILE: &str = ".rivus-generation.json";
const RVS_PRIMARY_PACKAGE_TARGETS_FILE: &str = ".rivus-primary-package-targets";
const RVS_RUN_GENERATION_SCHEMA_VERSION: u32 = 6;
const RVS_PRIMARY_PACKAGE_TARGETS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Snafu)]
pub(crate) enum RunGenerationError {
    #[snafu(display("cannot canonicalize project root {}: {source}", path.display()))]
    CanonicalizeProject {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot create run generation directory {}: {source}", path.display()))]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot canonicalize run generation directory {}: {source}", path.display()))]
    CanonicalizeRunsDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("run generation marker must be a regular file: {}", path.display()))]
    MarkerNotFile { path: PathBuf },
    #[snafu(display("cannot read run generation marker {}: {source}", path.display()))]
    ReadMarker {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("invalid run generation marker {}: {source}", path.display()))]
    ParseMarker {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[snafu(display("unsupported run generation marker version {actual} in {}; expected {expected}", path.display()))]
    MarkerVersion {
        path: PathBuf,
        actual: u32,
        expected: u32,
    },
    #[snafu(display("run generation marker identity does not match directory {}", path.display()))]
    MarkerIdentity { path: PathBuf },
    #[snafu(display("cannot serialize run generation marker for {}: {source}", path.display()))]
    SerializeMarker {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[snafu(display("cannot create run generation marker {}: {source}", path.display()))]
    CreateMarker {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot write run generation marker {}: {source}", path.display()))]
    WriteMarker {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot sync run generation marker {}: {source}", path.display()))]
    SyncMarker {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot remove owned run generation {}: {source}", path.display()))]
    RemoveGeneration {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Snafu)]
pub(crate) enum DriverProtocolError {
    #[snafu(display("Rivus driver protocol is missing {name}"))]
    MissingVariable { name: &'static str },
    #[snafu(display("Rivus driver protocol variable {name} must equal `1`, got {value:?}"))]
    InvalidFlag { name: &'static str, value: OsString },
    #[snafu(display(
        "Rivus driver protocol variable {name} must equal {expected:?}, got {value:?}"
    ))]
    InvalidValue {
        name: &'static str,
        expected: &'static str,
        value: OsString,
    },
    #[snafu(display("Rivus driver protocol variable {name} is not valid UTF-8: {value:?}"))]
    InvalidUtf8 { name: &'static str, value: OsString },
    #[snafu(display("Rivus driver protocol unexpectedly contains {name}"))]
    UnexpectedVariable { name: &'static str },
    #[snafu(display("Rivus driver generation is invalid: {source}"))]
    Generation { source: RunGenerationError },
    #[snafu(display("cannot canonicalize Rivus driver generation root {}: {source}", path.display()))]
    CanonicalizeGenerationRoot {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Rivus driver generation root is not its canonical path: {}", path.display()))]
    NonCanonicalGenerationRoot { path: PathBuf },
    #[snafu(display(
        "Rivus driver generation identity {actual:?} does not match marker identity {expected:?}"
    ))]
    GenerationIdentityMismatch { actual: String, expected: String },
    #[snafu(display("cannot read primary-package target authority {}: {source}", path.display()))]
    ReadPrimaryPackageTargets {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("invalid primary-package target authority {}: {source}", path.display()))]
    ParsePrimaryPackageTargets {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[snafu(display("unsupported primary-package target authority version {actual} in {}; expected {expected}", path.display()))]
    PrimaryPackageTargetsVersion {
        path: PathBuf,
        actual: u32,
        expected: u32,
    },
    #[snafu(display("primary-package target authority is invalid: {message}"))]
    InvalidPrimaryPackageTargets { message: String },
    #[snafu(display("cannot resolve the Rivus driver working directory: {source}"))]
    DriverWorkingDirectory { source: std::io::Error },
    #[snafu(display("Rivus driver protocol path for {name} must be {}, got {}", expected.display(), actual.display()))]
    PathMismatch {
        name: &'static str,
        expected: PathBuf,
        actual: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RunGenerationTargetScope {
    Production,
    WithTestExampleBench,
}

impl From<CargoTargetScope> for RunGenerationTargetScope {
    fn from(value: CargoTargetScope) -> Self {
        match value {
            CargoTargetScope::Production => Self::Production,
            CargoTargetScope::WithTestExampleBench => Self::WithTestExampleBench,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RunGenerationCollectionMode {
    Workspace,
    AllCrates,
    StandardLibrary,
}

/// Whether the collection compile also runs the direct lints. `check` is
/// the lint-bearing collection of `cargo rivus check`; its non-zero exit
/// short-circuits the command before graph analysis. `silent` serves the
/// analysis-only commands (report/why/infer) and suppresses diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CollectionLints {
    Silent,
    Check,
}

impl From<CallgraphCollectionMode> for RunGenerationCollectionMode {
    fn from(value: CallgraphCollectionMode) -> Self {
        match value {
            CallgraphCollectionMode::Workspace => Self::Workspace,
            CallgraphCollectionMode::AllCrates => Self::AllCrates,
            CallgraphCollectionMode::StandardLibrary => Self::StandardLibrary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "input", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RunGenerationAnalysisMode {
    ProjectCaps,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RunGenerationMode {
    Analysis {
        target_scope: RunGenerationTargetScope,
        analysis: RunGenerationAnalysisMode,
    },
    Collection {
        collection: RunGenerationCollectionMode,
        target_scope: RunGenerationTargetScope,
        lints: CollectionLints,
    },
}

impl RunGenerationMode {
    const fn rvs_name(&self) -> &'static str {
        match self {
            Self::Analysis { .. } => "analysis",
            Self::Collection {
                collection: RunGenerationCollectionMode::Workspace,
                lints: CollectionLints::Silent,
                ..
            } => "collection-workspace",
            Self::Collection {
                collection: RunGenerationCollectionMode::Workspace,
                lints: CollectionLints::Check,
                ..
            } => "collection-workspace-check",
            Self::Collection {
                collection: RunGenerationCollectionMode::AllCrates,
                lints: CollectionLints::Silent,
                ..
            } => "collection-all-crates",
            Self::Collection {
                collection: RunGenerationCollectionMode::AllCrates,
                lints: CollectionLints::Check,
                ..
            } => "collection-all-crates-check",
            Self::Collection {
                collection: RunGenerationCollectionMode::StandardLibrary,
                ..
            } => "collection-std",
        }
    }

    const fn rvs_target_name(&self) -> &'static str {
        let target_scope = match self {
            Self::Analysis { target_scope, .. } | Self::Collection { target_scope, .. } => {
                target_scope
            }
        };
        match target_scope {
            RunGenerationTargetScope::Production => "production",
            RunGenerationTargetScope::WithTestExampleBench => "all-targets",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunGenerationMarker {
    schema_version: u32,
    generation_id: String,
    project_root: PathBuf,
    mode: RunGenerationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimaryPackageTarget {
    crate_name: String,
    source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimaryPackageTargets {
    schema_version: u32,
    generation_id: String,
    targets: BTreeSet<PrimaryPackageTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadataOutput {
    packages: Vec<CargoMetadataPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadataPackage {
    manifest_path: PathBuf,
    targets: Vec<CargoMetadataTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadataTarget {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct RivusCallgraphOutput {
    pub(crate) generation_id: String,
    pub(crate) artifact_dir: PathBuf,
    pub(crate) crate_provenance: CrateProvenance,
}

impl RivusCallgraphOutput {
    pub(crate) fn rvs_write_artifact_file_no_replace_BIST(
        &self,
        file_name: &str,
        contents: &[u8],
    ) -> std::io::Result<()> {
        rvs_write_artifact_no_replace_BIST(&self.artifact_dir, file_name, contents)
    }

    #[cfg(test)]
    pub(super) fn rvs_for_test_BIS(
        generation_id: &str,
        artifact_dir: &Path,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(artifact_dir)?;
        Ok(Self {
            generation_id: generation_id.to_string(),
            artifact_dir: artifact_dir.to_path_buf(),
            crate_provenance: CrateProvenance::LegacyUnknown,
        })
    }
}

fn rvs_write_artifact_no_replace_BIST(
    artifact_dir: &Path,
    file_name: &str,
    contents: &[u8],
) -> std::io::Result<()> {
    if file_name.is_empty()
        || Path::new(file_name).components().count() != 1
        || !matches!(
            Path::new(file_name).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact file name must be one normal path segment",
        ));
    }
    let target_path = artifact_dir.join(file_name);
    let mut temp_file = tempfile::NamedTempFile::new_in(artifact_dir)?;
    temp_file.write_all(contents)?;
    temp_file.as_file().sync_all()?;
    temp_file
        .persist_noclobber(&target_path)
        .map_err(|error| error.error)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct RivusOfflineDriverInput {
    pub(crate) emissions: PathBuf,
    pub(crate) acknowledgement_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) enum RivusDriverMode {
    ProjectCaps {
        capsmap: Option<PathBuf>,
    },
    Offline(RivusOfflineDriverInput),
    Callgraph {
        output: RivusCallgraphOutput,
        lints: CollectionLints,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct RivusDriverConfig {
    pub(crate) mode: RivusDriverMode,
    pub(crate) ui_testing: bool,
}

#[derive(Debug, Clone, Default)]
struct DriverProtocolEnvironment {
    enabled: Option<OsString>,
    wrapper: Option<OsString>,
    generation_id: Option<OsString>,
    generation_root: Option<OsString>,
    callgraph: Option<OsString>,
    callgraph_dir: Option<OsString>,
    crate_provenance: Option<OsString>,
    capsmap: Option<OsString>,
    offline_caps: Option<OsString>,
    untested_paths: Option<OsString>,
    offline_emissions: Option<OsString>,
    offline_emissions_ack_dir: Option<OsString>,
    ui_testing: Option<OsString>,
    rustc_arguments: Vec<OsString>,
}

impl DriverProtocolEnvironment {
    fn rvs_from_current_BS() -> Self {
        Self {
            enabled: env::var_os("RIVUS_ENABLED"),
            wrapper: env::var_os("RIVUS_WRAPPER"),
            generation_id: env::var_os("RIVUS_GENERATION_ID"),
            generation_root: env::var_os("RIVUS_GENERATION_ROOT"),
            callgraph: env::var_os("RIVUS_CALLGRAPH"),
            callgraph_dir: env::var_os("RIVUS_CALLGRAPH_DIR"),
            crate_provenance: env::var_os("RIVUS_CRATE_PROVENANCE"),
            capsmap: env::var_os("RIVUS_CAPSMAP"),
            offline_caps: env::var_os("RIVUS_OFFLINE_CAPS"),
            untested_paths: env::var_os("RIVUS_UNTESTED_PATHS"),
            offline_emissions: env::var_os("RIVUS_OFFLINE_EMISSIONS"),
            offline_emissions_ack_dir: env::var_os("RIVUS_OFFLINE_EMISSIONS_ACK_DIR"),
            ui_testing: env::var_os("RIVUS_UI_TESTING"),
            rustc_arguments: env::args_os().collect(),
        }
    }

    const fn rvs_contains_rivus_authority(&self) -> bool {
        self.wrapper.is_some()
            || self.generation_id.is_some()
            || self.generation_root.is_some()
            || self.callgraph.is_some()
            || self.callgraph_dir.is_some()
            || self.crate_provenance.is_some()
            || self.capsmap.is_some()
            || self.offline_caps.is_some()
            || self.untested_paths.is_some()
            || self.offline_emissions.is_some()
            || self.offline_emissions_ack_dir.is_some()
            || self.ui_testing.is_some()
    }
}

fn rvs_require_driver_flag(
    value: Option<&OsString>,
    name: &'static str,
) -> Result<(), DriverProtocolError> {
    match value {
        Some(value) if value == "1" => Ok(()),
        Some(value) => Err(DriverProtocolError::InvalidFlag {
            name,
            value: value.clone(),
        }),
        None => Err(DriverProtocolError::MissingVariable { name }),
    }
}

fn rvs_optional_driver_flag(
    value: Option<&OsString>,
    name: &'static str,
) -> Result<bool, DriverProtocolError> {
    match value {
        Some(value) if value == "1" => Ok(true),
        Some(value) => Err(DriverProtocolError::InvalidFlag {
            name,
            value: value.clone(),
        }),
        None => Ok(false),
    }
}

fn rvs_require_driver_utf8(
    value: Option<&OsString>,
    name: &'static str,
) -> Result<String, DriverProtocolError> {
    let value = value.ok_or(DriverProtocolError::MissingVariable { name })?;
    value
        .clone()
        .into_string()
        .map_err(|value| DriverProtocolError::InvalidUtf8 { name, value })
}

fn rvs_require_driver_path(
    value: Option<&OsString>,
    name: &'static str,
) -> Result<PathBuf, DriverProtocolError> {
    value
        .map(PathBuf::from)
        .ok_or(DriverProtocolError::MissingVariable { name })
}

const fn rvs_reject_driver_variable(
    value: Option<&OsString>,
    name: &'static str,
) -> Result<(), DriverProtocolError> {
    if value.is_some() {
        Err(DriverProtocolError::UnexpectedVariable { name })
    } else {
        Ok(())
    }
}

fn rvs_require_driver_path_match(
    value: Option<&OsString>,
    name: &'static str,
    expected: &Path,
) -> Result<PathBuf, DriverProtocolError> {
    let actual = rvs_require_driver_path(value, name)?;
    if actual != expected {
        return Err(DriverProtocolError::PathMismatch {
            name,
            expected: expected.to_path_buf(),
            actual,
        });
    }
    Ok(expected.to_path_buf())
}

fn rvs_read_primary_package_targets_BIS(
    root: &Path,
    generation_id: &str,
) -> Result<PrimaryPackageTargets, DriverProtocolError> {
    let path = root.join(RVS_PRIMARY_PACKAGE_TARGETS_FILE);
    let json = super::fs_guard::rvs_read_file_utf8_BIS(&path).map_err(|source| {
        DriverProtocolError::ReadPrimaryPackageTargets {
            path: path.clone(),
            source,
        }
    })?;
    let targets: PrimaryPackageTargets = serde_json::from_str(&json).map_err(|source| {
        DriverProtocolError::ParsePrimaryPackageTargets {
            path: path.clone(),
            source,
        }
    })?;
    if targets.schema_version != RVS_PRIMARY_PACKAGE_TARGETS_SCHEMA_VERSION {
        return Err(DriverProtocolError::PrimaryPackageTargetsVersion {
            path,
            actual: targets.schema_version,
            expected: RVS_PRIMARY_PACKAGE_TARGETS_SCHEMA_VERSION,
        });
    }
    if targets.generation_id != generation_id {
        return Err(DriverProtocolError::InvalidPrimaryPackageTargets {
            message: format!(
                "generation identity {:?} does not match {:?}",
                targets.generation_id, generation_id
            ),
        });
    }
    if targets.targets.is_empty() {
        return Err(DriverProtocolError::InvalidPrimaryPackageTargets {
            message: "target set must not be empty".to_string(),
        });
    }
    if let Some(target) = targets
        .targets
        .iter()
        .find(|target| target.crate_name.is_empty() || !target.source_path.is_absolute())
    {
        return Err(DriverProtocolError::InvalidPrimaryPackageTargets {
            message: format!("invalid target record: {target:?}"),
        });
    }
    Ok(targets)
}

fn rvs_driver_crate_provenance_BIS(
    root: &Path,
    generation_id: &str,
    arguments: &[OsString],
) -> Result<CrateProvenance, DriverProtocolError> {
    let targets = rvs_read_primary_package_targets_BIS(root, generation_id)?;
    let mut crate_names = BTreeSet::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let Some(argument) = arguments.get(index) else {
            break;
        };
        if argument == "--crate-name" {
            if let Some(name) = arguments.get(index.saturating_add(1)) {
                crate_names.insert(name.to_string_lossy().into_owned());
            }
            index = index.saturating_add(2);
            continue;
        }
        if let Some(name) = argument
            .to_str()
            .and_then(|argument| argument.strip_prefix("--crate-name="))
        {
            crate_names.insert(name.to_string());
        }
        index = index.saturating_add(1);
    }
    let Some(crate_name) = crate_names.first() else {
        return Ok(CrateProvenance::Dependency);
    };
    if crate_names.len() != 1 {
        return Ok(CrateProvenance::Dependency);
    }
    let matching_sources = targets
        .targets
        .iter()
        .filter(|target| target.crate_name == *crate_name)
        .map(|target| target.source_path.as_path())
        .collect::<BTreeSet<_>>();
    if matching_sources.is_empty() {
        return Ok(CrateProvenance::Dependency);
    }
    let working_directory = std::env::current_dir()
        .map_err(|source| DriverProtocolError::DriverWorkingDirectory { source })?;
    for argument in arguments {
        let argument_path = Path::new(argument);
        let candidate = if argument_path.is_absolute() {
            argument_path.to_path_buf()
        } else {
            working_directory.join(argument_path)
        };
        if let Ok(candidate) = candidate.canonicalize()
            && matching_sources.contains(candidate.as_path())
        {
            return Ok(CrateProvenance::PrimaryPackage);
        }
    }
    Ok(CrateProvenance::Dependency)
}

fn rvs_require_active_driver_generation(
    root: &Path,
    marker: &RunGenerationMarker,
) -> Result<(), DriverProtocolError> {
    let dir_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DriverProtocolError::Generation {
            source: RunGenerationError::MarkerIdentity {
                path: root.to_path_buf(),
            },
        })?;
    if dir_name != marker.generation_id {
        return Err(DriverProtocolError::GenerationIdentityMismatch {
            actual: dir_name.to_string(),
            expected: marker.generation_id.clone(),
        });
    }
    Ok(())
}

pub(crate) fn rvs_load_driver_protocol_BIS()
-> Result<Option<RivusDriverConfig>, DriverProtocolError> {
    let environment = DriverProtocolEnvironment::rvs_from_current_BS();
    if environment.enabled.is_none() {
        if environment.rvs_contains_rivus_authority() {
            return Err(DriverProtocolError::MissingVariable {
                name: "RIVUS_ENABLED",
            });
        }
        return Ok(None);
    }
    rvs_parse_driver_protocol_environment_BIS(&environment).map(Some)
}

fn rvs_parse_driver_protocol_environment_BIS(
    environment: &DriverProtocolEnvironment,
) -> Result<RivusDriverConfig, DriverProtocolError> {
    rvs_require_driver_flag(environment.enabled.as_ref(), "RIVUS_ENABLED")?;
    rvs_require_driver_flag(environment.wrapper.as_ref(), "RIVUS_WRAPPER")?;
    let ui_testing = rvs_optional_driver_flag(environment.ui_testing.as_ref(), "RIVUS_UI_TESTING")?;
    let generation_id =
        rvs_require_driver_utf8(environment.generation_id.as_ref(), "RIVUS_GENERATION_ID")?;
    let generation_root = rvs_require_driver_path(
        environment.generation_root.as_ref(),
        "RIVUS_GENERATION_ROOT",
    )?;
    let canonical_root = generation_root.canonicalize().map_err(|source| {
        DriverProtocolError::CanonicalizeGenerationRoot {
            path: generation_root.clone(),
            source,
        }
    })?;
    if canonical_root != generation_root {
        return Err(DriverProtocolError::NonCanonicalGenerationRoot {
            path: generation_root,
        });
    }
    let marker = rvs_read_run_generation_marker_BIS(&canonical_root)
        .map_err(|source| DriverProtocolError::Generation { source })?;
    if generation_id != marker.generation_id {
        return Err(DriverProtocolError::GenerationIdentityMismatch {
            actual: generation_id,
            expected: marker.generation_id,
        });
    }
    rvs_require_active_driver_generation(&canonical_root, &marker)?;
    let generation_project_root = marker.project_root.clone();

    let mode = match marker.mode {
        RunGenerationMode::Collection { lints, .. } => {
            rvs_require_driver_flag(environment.callgraph.as_ref(), "RIVUS_CALLGRAPH")?;
            let artifact_dir = rvs_require_driver_path_match(
                environment.callgraph_dir.as_ref(),
                "RIVUS_CALLGRAPH_DIR",
                &canonical_root.join("artifacts"),
            )?;
            match environment.crate_provenance.as_deref() {
                Some(value) if value == "cargo-primary" => {}
                Some(value) => {
                    return Err(DriverProtocolError::InvalidValue {
                        name: "RIVUS_CRATE_PROVENANCE",
                        expected: "cargo-primary",
                        value: value.to_owned(),
                    });
                }
                None => {
                    return Err(DriverProtocolError::MissingVariable {
                        name: "RIVUS_CRATE_PROVENANCE",
                    });
                }
            }
            for (value, name) in [
                (environment.capsmap.as_ref(), "RIVUS_CAPSMAP"),
                (environment.offline_caps.as_ref(), "RIVUS_OFFLINE_CAPS"),
                (environment.untested_paths.as_ref(), "RIVUS_UNTESTED_PATHS"),
                (
                    environment.offline_emissions.as_ref(),
                    "RIVUS_OFFLINE_EMISSIONS",
                ),
                (
                    environment.offline_emissions_ack_dir.as_ref(),
                    "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
                ),
            ] {
                rvs_reject_driver_variable(value, name)?;
            }
            RivusDriverMode::Callgraph {
                output: RivusCallgraphOutput {
                    generation_id: marker.generation_id,
                    artifact_dir,
                    crate_provenance: rvs_driver_crate_provenance_BIS(
                        &canonical_root,
                        &generation_id,
                        &environment.rustc_arguments,
                    )?,
                },
                lints,
            }
        }
        RunGenerationMode::Analysis {
            analysis: RunGenerationAnalysisMode::ProjectCaps,
            ..
        } => {
            for (value, name) in [
                (environment.callgraph.as_ref(), "RIVUS_CALLGRAPH"),
                (environment.callgraph_dir.as_ref(), "RIVUS_CALLGRAPH_DIR"),
                (
                    environment.crate_provenance.as_ref(),
                    "RIVUS_CRATE_PROVENANCE",
                ),
                (environment.offline_caps.as_ref(), "RIVUS_OFFLINE_CAPS"),
                (environment.untested_paths.as_ref(), "RIVUS_UNTESTED_PATHS"),
                (
                    environment.offline_emissions.as_ref(),
                    "RIVUS_OFFLINE_EMISSIONS",
                ),
                (
                    environment.offline_emissions_ack_dir.as_ref(),
                    "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
                ),
            ] {
                rvs_reject_driver_variable(value, name)?;
            }
            let capsmap = match environment.capsmap.as_ref() {
                Some(_) => Some(rvs_require_driver_path_match(
                    environment.capsmap.as_ref(),
                    "RIVUS_CAPSMAP",
                    &generation_project_root.join("caps"),
                )?),
                None => None,
            };
            RivusDriverMode::ProjectCaps { capsmap }
        }
        RunGenerationMode::Analysis {
            analysis: RunGenerationAnalysisMode::Offline,
            ..
        } => {
            rvs_require_driver_flag(environment.offline_caps.as_ref(), "RIVUS_OFFLINE_CAPS")?;
            for (value, name) in [
                (environment.callgraph.as_ref(), "RIVUS_CALLGRAPH"),
                (environment.callgraph_dir.as_ref(), "RIVUS_CALLGRAPH_DIR"),
                (
                    environment.crate_provenance.as_ref(),
                    "RIVUS_CRATE_PROVENANCE",
                ),
                (environment.capsmap.as_ref(), "RIVUS_CAPSMAP"),
                (environment.untested_paths.as_ref(), "RIVUS_UNTESTED_PATHS"),
            ] {
                rvs_reject_driver_variable(value, name)?;
            }
            let emissions = rvs_require_driver_path_match(
                environment.offline_emissions.as_ref(),
                "RIVUS_OFFLINE_EMISSIONS",
                &canonical_root.join("offline-emissions.json"),
            )?;
            let acknowledgement_dir = rvs_require_driver_path_match(
                environment.offline_emissions_ack_dir.as_ref(),
                "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
                &canonical_root.join("offline-emission-acks"),
            )?;
            RivusDriverMode::Offline(RivusOfflineDriverInput {
                emissions,
                acknowledgement_dir,
            })
        }
    };
    Ok(RivusDriverConfig { mode, ui_testing })
}

pub(crate) fn rvs_validate_optional_capsmap_dir_BIS(path: &Path) -> Result<bool, String> {
    super::fs_guard::rvs_validate_optional_dir_BIS(path, "capsmap path")
}

fn rvs_absolute_path_BIS(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map_err(|e| format!("current dir invalid: {e}"))
            .map(|cwd| cwd.join(path))
    }
}

/// Configuration for running `cargo check` with a Rivus driver mode.
#[derive(Debug)]
pub(crate) struct CargoCheckConfig<'a> {
    pub(crate) project_path: &'a Path,
    generation: &'a RivusRunGeneration,
    pub(crate) mode: CargoCheckMode,
    pub(crate) target_scope: CargoTargetScope,
    pub(crate) extra_args: Vec<&'a str>,
    pub(crate) target_subdir: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CargoCheckMode {
    Lint(CargoLintInput),
    Callgraph {
        collection: CallgraphCollectionMode,
        artifact_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CargoLintInput {
    Offline(OfflineLintInput),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfflineLintInput {
    pub(crate) emissions: OfflineEmissionInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfflineEmissionInput {
    pub(crate) path: PathBuf,
    pub(crate) acknowledgement_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallgraphCollectionMode {
    Workspace,
    AllCrates,
    StandardLibrary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CargoCheckError {
    Message(String),
    ExitCode(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CallgraphCollectionError {
    Cargo(CargoCheckError),
    Artifact(String),
}

#[derive(Debug)]
struct CollectedCallgraph {
    callgraph: FnGraph,
}

#[derive(Debug)]
struct RivusRunGeneration {
    temp_dir: Option<tempfile::TempDir>,
    root: PathBuf,
    artifact_dir: PathBuf,
    generation_id: String,
    target_subdir: String,
}

impl RivusRunGeneration {
    fn rvs_root(&self) -> &Path {
        &self.root
    }
    fn rvs_artifact_dir(&self) -> &Path {
        &self.artifact_dir
    }
    fn rvs_target_subdir(&self) -> &str {
        &self.target_subdir
    }
    fn rvs_generation_id(&self) -> &str {
        &self.generation_id
    }

    fn rvs_cleanup_BIMS(&mut self) -> Result<(), RunGenerationError> {
        if let Some(temp_dir) = self.temp_dir.take() {
            let root = self.root.clone();
            temp_dir
                .close()
                .map_err(|source| RunGenerationError::RemoveGeneration { path: root, source })?;
        }
        Ok(())
    }
}

#[allow(
    clippy::allow_attributes,
    reason = "generation directories must be removed during panic unwinding"
)]
impl Drop for RivusRunGeneration {
    fn drop(&mut self) {
        if let Some(temp_dir) = self.temp_dir.take()
            && let Err(error) = temp_dir.close()
        {
            eprintln!(
                "warning: cannot clean Rivus run generation {} during drop: {error}",
                self.root.display()
            );
        }
    }
}

impl CargoCheckError {
    pub(crate) const fn rvs_exit_code(&self) -> i32 {
        match self {
            Self::Message(_) => 1,
            Self::ExitCode(code) => *code,
        }
    }
}

impl fmt::Display for CargoCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::ExitCode(code) => write!(f, "cargo check failed (exit code {code})"),
        }
    }
}

impl fmt::Display for CallgraphCollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cargo(error) => error.fmt(f),
            Self::Artifact(message) => f.write_str(message),
        }
    }
}

fn rvs_create_primary_package_targets_file_BIST(
    generation_root: &Path,
    targets: &PrimaryPackageTargets,
) -> Result<(), String> {
    let path = generation_root.join(RVS_PRIMARY_PACKAGE_TARGETS_FILE);
    let json = serde_json::to_vec(targets).map_err(|error| {
        format!(
            "cannot serialize primary-package target authority {}: {error}",
            path.display()
        )
    })?;
    super::fs_guard::rvs_atomic_write_BIST(&path, &json)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn rvs_cargo_target_is_selected(
    target: &CargoMetadataTarget,
    target_scope: CargoTargetScope,
) -> bool {
    target_scope == CargoTargetScope::WithTestExampleBench
        || !target
            .kind
            .iter()
            .any(|kind| matches!(kind.as_str(), "test" | "example" | "bench"))
}

fn rvs_write_primary_package_targets_BIST(
    project_path: &Path,
    generation: &RivusRunGeneration,
    target_scope: CargoTargetScope,
) -> Result<(), String> {
    let manifest_path = project_path.join("Cargo.toml");
    let canonical_manifest = manifest_path.canonicalize().map_err(|error| {
        format!(
            "cannot canonicalize primary-package manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let output = Command::new(rvs_cargo_command_from_env_BS())
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(&canonical_manifest)
        .current_dir(project_path)
        .output()
        .map_err(|error| format!("cannot run cargo metadata for target provenance: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata for target provenance failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata: CargoMetadataOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cannot parse cargo metadata for target provenance: {error}"))?;
    let mut selected_package = None;
    for package in metadata.packages {
        let package_manifest = package.manifest_path.canonicalize().map_err(|error| {
            format!(
                "cannot canonicalize cargo metadata manifest {}: {error}",
                package.manifest_path.display()
            )
        })?;
        if package_manifest == canonical_manifest && selected_package.replace(package).is_some() {
            return Err(format!(
                "cargo metadata contains duplicate package records for {}",
                canonical_manifest.display()
            ));
        }
    }
    let package = selected_package.ok_or_else(|| {
        format!(
            "cargo metadata does not contain the primary package {}",
            canonical_manifest.display()
        )
    })?;
    let mut targets = BTreeSet::new();
    for target in package.targets {
        if !rvs_cargo_target_is_selected(&target, target_scope) {
            continue;
        }
        let source_path = target.src_path.canonicalize().map_err(|error| {
            format!(
                "cannot canonicalize primary-package target source {}: {error}",
                target.src_path.display()
            )
        })?;
        targets.insert(PrimaryPackageTarget {
            crate_name: CrateName::rvs_from_manifest_name(&target.name)
                .rvs_as_str()
                .to_string(),
            source_path,
        });
    }
    if targets.is_empty() {
        return Err(format!(
            "cargo metadata contains no selected targets for {}",
            canonical_manifest.display()
        ));
    }
    rvs_create_primary_package_targets_file_BIST(
        generation.rvs_root(),
        &PrimaryPackageTargets {
            schema_version: RVS_PRIMARY_PACKAGE_TARGETS_SCHEMA_VERSION,
            generation_id: generation.rvs_generation_id().to_string(),
            targets,
        },
    )
}

/// Runs `cargo check` with the rivus lint pass configured according to `config`.
/// Returns `Ok(())` on success, `Err(message)` on failure.
///
/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
pub(crate) fn rvs_run_cargo_check_impl_BIST(
    config: &CargoCheckConfig,
) -> Result<(), CargoCheckError> {
    let mut cmd = rvs_prepare_cargo_check_command_BIST(config)?;
    let exit_status = cmd
        .spawn()
        .map_err(|e| CargoCheckError::Message(format!("could not run cargo: {e}")))?
        .wait()
        .map_err(|e| CargoCheckError::Message(format!("failed to wait for cargo: {e}")))?;
    if !exit_status.success() {
        return Err(CargoCheckError::ExitCode(exit_status.code().unwrap_or(1)));
    }
    Ok(())
}

fn rvs_prepare_cargo_check_command_BIST(
    config: &CargoCheckConfig,
) -> Result<Command, CargoCheckError> {
    let self_path = rvs_current_wrapper_exe_BIS()
        .map_err(|e| CargoCheckError::Message(format!("current executable path invalid: {e}")))?;
    let cargo = rvs_cargo_command_from_env_BS();
    let mut cmd = Command::new(&cargo);
    let project_path =
        rvs_absolute_path_BIS(config.project_path).map_err(CargoCheckError::Message)?;

    if matches!(config.mode, CargoCheckMode::Callgraph { .. }) {
        rvs_write_primary_package_targets_BIST(
            &project_path,
            config.generation,
            config.target_scope,
        )
        .map_err(CargoCheckError::Message)?;
    }

    for key in [
        "RIVUS_CALLGRAPH",
        "RIVUS_CALLGRAPH_DIR",
        "RIVUS_CRATE_PROVENANCE",
        "RIVUS_CAPSMAP",
        "RIVUS_OFFLINE_EMISSIONS",
        "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
        "RIVUS_OFFLINE_CAPS",
        "RIVUS_UI_TESTING",
        "RIVUS_UNTESTED_PATHS",
        "RIVUS_ENABLED",
        "RIVUS_WRAPPER",
        "RIVUS_GENERATION_ID",
        "RIVUS_GENERATION_ROOT",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_PRIMARY_PACKAGE",
    ] {
        cmd.env_remove(key);
    }

    let build_std = matches!(
        config.mode,
        CargoCheckMode::Callgraph {
            collection: CallgraphCollectionMode::StandardLibrary,
            ..
        }
    );
    if build_std {
        cmd.env("RUSTUP_TOOLCHAIN", "nightly");
    }
    cmd.current_dir(&project_path);

    let wrapper_env = match &config.mode {
        CargoCheckMode::Lint(_)
        | CargoCheckMode::Callgraph {
            collection: CallgraphCollectionMode::Workspace,
            ..
        } => "RUSTC_WORKSPACE_WRAPPER",
        CargoCheckMode::Callgraph {
            collection:
                CallgraphCollectionMode::AllCrates | CallgraphCollectionMode::StandardLibrary,
            ..
        } => "RUSTC_WRAPPER",
    };
    cmd.env(wrapper_env, &self_path)
        .env("RIVUS_ENABLED", "1")
        .env("RIVUS_WRAPPER", "1");
    cmd.env("RIVUS_GENERATION_ID", config.generation.rvs_generation_id())
        .env("RIVUS_GENERATION_ROOT", config.generation.rvs_root());

    match &config.mode {
        CargoCheckMode::Lint(CargoLintInput::Offline(input)) => {
            cmd.env("RIVUS_OFFLINE_CAPS", "1");
            cmd.env("RIVUS_OFFLINE_EMISSIONS", &input.emissions.path);
            cmd.env(
                "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
                &input.emissions.acknowledgement_dir,
            );
        }
        CargoCheckMode::Callgraph { artifact_dir, .. } => {
            cmd.env("RIVUS_CALLGRAPH", "1");
            cmd.env("RIVUS_CALLGRAPH_DIR", artifact_dir);
            cmd.env("RIVUS_CRATE_PROVENANCE", "cargo-primary");
        }
    }

    cmd.arg("check");
    let needs_test_profile = match &config.mode {
        CargoCheckMode::Lint(_) => true,
        CargoCheckMode::Callgraph {
            collection: CallgraphCollectionMode::Workspace,
            ..
        } => true,
        CargoCheckMode::Callgraph {
            collection:
                CallgraphCollectionMode::AllCrates | CallgraphCollectionMode::StandardLibrary,
            ..
        } => false,
    };
    if needs_test_profile {
        cmd.arg("--profile").arg("test");
    }
    if let Some(arg) = config.target_scope.rvs_cargo_check_arg() {
        cmd.arg(arg);
    }
    if build_std {
        cmd.arg("-Zbuild-std=std,core,alloc");
        cmd.arg("--target").arg(rustc_session::config::host_tuple());
    }
    if let Some(subdir) = config.target_subdir {
        let target_dir = project_path.join("target").join(subdir);
        cmd.arg("--target-dir").arg(&target_dir);
    }
    for arg in &config.extra_args {
        cmd.arg(arg);
    }
    Ok(cmd)
}

fn rvs_cargo_command_from_env_BS() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

pub(crate) fn rvs_current_wrapper_exe_BIS() -> Result<PathBuf, std::io::Error> {
    let current = env::current_exe()?;
    if let Some(parent) = current.parent()
        && parent.file_name().is_some_and(|name| name == "deps")
        && let Some(debug_dir) = parent.parent()
    {
        let candidate = debug_dir.join("cargo-rivus");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Ok(current)
}

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
pub(crate) fn rvs_run_cargo_check_BIST(extra_args: &[String]) -> Result<(), i32> {
    rvs_run_cargo_check_at_BIST(Path::new("."), extra_args)
}

/// The lint-bearing pipeline of `cargo rivus check` for an explicit project
/// path: the collection compile runs the direct lints, and its non-zero
/// exit short-circuits the command before graph analysis.
fn rvs_run_cargo_check_at_BIST(project_path: &Path, extra_args: &[String]) -> Result<(), i32> {
    if let Err(e) = rvs_reject_forwarded_check_args(extra_args) {
        eprintln!("{e}");
        return Err(2);
    }
    let target_scope = CargoTargetScope::WithTestExampleBench;
    let extra_args_ref: Vec<&str> = extra_args.iter().map(|arg| arg.as_str()).collect();
    let caps = match rvs_load_project_caps_BIS(project_path) {
        Ok(caps) => caps,
        Err(e) => {
            eprintln!("offline caps check cannot load caps/: {e}");
            return Err(1);
        }
    };
    let local_crate_names = match rvs_load_local_crate_prefixes_BIS(project_path, target_scope) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("offline caps check cannot detect local crates: {e}");
            return Err(1);
        }
    };
    let callgraph = match rvs_collect_callgraph_with_args_detailed_BIST(
        project_path,
        CallgraphCollectionMode::Workspace,
        target_scope,
        extra_args_ref.clone(),
        &local_crate_names,
        CollectionLints::Check,
    ) {
        Ok(collected) => collected.callgraph,
        Err(CallgraphCollectionError::Cargo(CargoCheckError::ExitCode(code))) => {
            // The lint-bearing collection compile failed and already
            // surfaced its diagnostics; graph analysis is skipped.
            eprintln!(
                "rivus check stopped: the collection compile failed with exit code {code}; \
                 fix the diagnostics above before graph analysis runs"
            );
            return Err(code);
        }
        Err(error) => {
            eprintln!("offline caps check unavailable: {error}");
            return Err(1);
        }
    };
    // One fixed-point pass feeds both coverage selection and diagnostics.
    let analysis =
        crate::inference::PreparedLocalAnalysis::rvs_prepare(&callgraph, &caps, &local_crate_names);
    let uncovered = crate::offline_caps::rvs_uncovered_test_functions(
        &callgraph,
        &analysis,
        &local_crate_names,
    );
    let report = crate::offline_caps::rvs_check_offline_caps_with_analysis(
        &callgraph,
        &analysis,
        &caps,
        &local_crate_names,
    );
    let mut offline_emissions = report.rvs_emissions(&callgraph);
    offline_emissions.extend(crate::offline_caps::rvs_untested_emissions(&uncovered));
    let lint_result = rvs_run_project_lints_BIST(
        project_path,
        &target_scope,
        &extra_args_ref,
        &offline_emissions,
    );
    if let Err(error) = lint_result {
        eprintln!("{error}");
        return Err(error.rvs_exit_code());
    }
    println!("Offline Caps Check: ok");
    Ok(())
}

/// The replay compile of `cargo rivus check`: it anchors the merged-graph
/// emissions (contract diagnostics and the untested selection) onto real
/// source spans. Always runs in Offline mode, even with zero emissions.
fn rvs_run_project_lints_BIST(
    project_path: &Path,
    target_scope: &CargoTargetScope,
    extra_args: &[&str],
    offline_emissions: &[crate::offline_caps::OfflineCapsEmission],
) -> Result<(), CargoCheckError> {
    let mut generation = rvs_reserve_run_generation_for_BIST(
        project_path,
        RunGenerationMode::Analysis {
            target_scope: (*target_scope).into(),
            analysis: RunGenerationAnalysisMode::Offline,
        },
    )
    .map_err(|error| CargoCheckError::Message(error.to_string()))?;
    let lint_result = (|| {
        let path = rvs_write_offline_emissions_BIST(generation.rvs_root(), offline_emissions)
            .map_err(CargoCheckError::Message)?;
        let ack_dir = generation.rvs_root().join("offline-emission-acks");
        std::fs::create_dir(&ack_dir).map_err(|error| {
            CargoCheckError::Message(format!(
                "cannot create offline emission acknowledgement directory {}: {error}",
                ack_dir.display()
            ))
        })?;
        let lint_input = CargoLintInput::Offline(OfflineLintInput {
            emissions: OfflineEmissionInput {
                path,
                acknowledgement_dir: ack_dir,
            },
        });
        let cargo_result = rvs_run_cargo_check_impl_BIST(&CargoCheckConfig {
            project_path,
            generation: &generation,
            mode: CargoCheckMode::Lint(lint_input),
            target_scope: *target_scope,
            extra_args: extra_args.to_vec(),
            target_subdir: Some(generation.rvs_target_subdir()),
        });
        let ack_result = if cargo_result.is_ok() {
            rvs_verify_offline_emission_acks_BIS(generation.rvs_root(), offline_emissions).map(Some)
        } else {
            Ok(None)
        };
        rvs_merge_lint_results(&cargo_result, &ack_result)
    })();
    let cleanup_result = rvs_cleanup_run_generation_BIMS(&mut generation);
    match (lint_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(cleanup)) => Err(CargoCheckError::Message(cleanup)),
        (Err(error), Err(cleanup)) => {
            eprintln!("warning: additionally failed to clean lint generation: {cleanup}");
            Err(error)
        }
    }
}

fn rvs_merge_lint_results(
    cargo_result: &Result<(), CargoCheckError>,
    ack_result: &Result<Option<VerifiedOfflineEmissionAcks>, String>,
) -> Result<(), CargoCheckError> {
    match (cargo_result, ack_result) {
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(message)) => Err(CargoCheckError::Message(message.clone())),
        (Err(error), _) => Err(error.clone()),
    }
}

fn rvs_write_offline_emissions_BIST(
    generation_root: &Path,
    emissions: &[crate::offline_caps::OfflineCapsEmission],
) -> Result<PathBuf, String> {
    let path = generation_root.join("offline-emissions.json");
    let json = crate::offline_caps::rvs_serialize_emissions(emissions)?;
    super::fs_guard::rvs_atomic_write_BIST(&path, json.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedOfflineEmissionAcks;

fn rvs_verify_offline_emission_acks_BIS(
    generation_root: &Path,
    emissions: &[crate::offline_caps::OfflineCapsEmission],
) -> Result<VerifiedOfflineEmissionAcks, String> {
    let ack_dir = generation_root.join("offline-emission-acks");
    for (emission_index, emission) in emissions.iter().enumerate() {
        for (anchor_index, anchor) in emission.span_anchors.iter().enumerate() {
            let ack = ack_dir.join(crate::offline_caps::rvs_emission_ack_name(
                emission_index,
                anchor_index,
            ));
            if !ack.is_file() {
                return Err(format!(
                    "offline caps diagnostic was not matched by the final compilation: {} (crate id {})",
                    anchor.identity.def_path, anchor.identity.crate_id
                ));
            }
        }
    }
    Ok(VerifiedOfflineEmissionAcks)
}

fn rvs_reject_forwarded_check_args(extra_args: &[String]) -> Result<(), String> {
    let mut index = 0usize;
    while index < extra_args.len() {
        let Some(arg) = extra_args.get(index) else {
            break;
        };
        if arg == "--manifest-path" || arg.starts_with("--manifest-path=") {
            return Err("cargo rivus check does not support forwarded --manifest-path; run from the target project directory or use commands with an explicit path".into());
        }
        if arg == "--target-dir" || arg.starts_with("--target-dir=") {
            return Err("cargo rivus check does not support forwarded --target-dir because check uses an isolated target directory".into());
        }
        if matches!(
            arg.as_str(),
            "--lib"
                | "--bins"
                | "--bin"
                | "--examples"
                | "--example"
                | "--tests"
                | "--test"
                | "--benches"
                | "--bench"
                | "--all-targets"
        ) || arg.starts_with("--bin=")
            || arg.starts_with("--example=")
            || arg.starts_with("--test=")
            || arg.starts_with("--bench=")
        {
            return Err(format!(
                "cargo rivus check does not support forwarded target selector '{arg}'; check analyzes its fixed all-targets universe"
            ));
        }
        if matches!(
            arg.as_str(),
            "--help" | "-h" | "--version" | "-V" | "--unit-graph" | "--build-plan"
        ) || arg == "--print"
            || arg.starts_with("--print=")
        {
            return Err(format!(
                "cargo rivus check does not support forwarded non-building mode '{arg}'"
            ));
        }
        if arg == "--message-format" {
            let Some(value) = extra_args.get(index + 1) else {
                return Err(
                    "cargo rivus check does not support empty forwarded --message-format".into(),
                );
            };
            rvs_reject_json_message_format(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--message-format=") {
            rvs_reject_json_message_format(value)?;
        }
        if matches!(arg.as_str(), "--workspace" | "--all" | "--exclude")
            || arg.starts_with("--exclude=")
            || arg == "--package"
            || arg.starts_with("--package=")
            || arg == "-p"
            || (arg.starts_with("-p") && arg.len() > 2)
        {
            return Err(format!(
                "cargo rivus check does not support forwarded workspace package selector '{arg}'; run from the package directory"
            ));
        }
        if arg == "--config" {
            let Some(value) = extra_args.get(index + 1) else {
                return Err("cargo rivus check does not support empty forwarded --config".into());
            };
            rvs_reject_dangerous_forwarded_config(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            rvs_reject_dangerous_forwarded_config(value)?;
        }
        index += 1;
    }
    Ok(())
}

fn rvs_reject_json_message_format(value: &str) -> Result<(), String> {
    if value.split(',').any(|format| format.starts_with("json")) {
        return Err(format!(
            "cargo rivus check does not support forwarded JSON message format '{value}'"
        ));
    }
    Ok(())
}

fn rvs_reject_dangerous_forwarded_config(value: &str) -> Result<(), String> {
    let dangerous_keys = [
        "build.rustc",
        "build.rustc-wrapper",
        "build.rustc-workspace-wrapper",
        "env.RIVUS_ENABLED",
        "env.RIVUS_WRAPPER",
        "env.RIVUS_CAPSMAP",
        "env.RIVUS_CALLGRAPH",
        "env.RIVUS_CALLGRAPH_DIR",
        "env.RIVUS_CRATE_PROVENANCE",
        "env.RIVUS_OFFLINE_EMISSIONS",
        "env.RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
        "env.RIVUS_OFFLINE_CAPS",
        "env.RIVUS_UI_TESTING",
        "env.RIVUS_UNTESTED_PATHS",
        "env.RUSTC",
        "env.RUSTC_WRAPPER",
        "env.RUSTC_WORKSPACE_WRAPPER",
    ];
    if !value.contains('=') {
        return Err("cargo rivus check does not support forwarded --config paths; pass cargo config through the environment instead".into());
    }
    let doc = value
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("cannot parse forwarded --config value: {e}"))?;
    let mut key_paths = Vec::new();
    for (key, item) in doc.iter() {
        rvs_collect_config_key_paths_M(key, item, &mut key_paths);
    }
    for key in key_paths {
        if dangerous_keys
            .iter()
            .any(|dangerous| key == *dangerous || key.starts_with(&format!("{dangerous}.")))
        {
            return Err(format!(
                "cargo rivus check does not support forwarded --config for driver-controlled key '{key}'"
            ));
        }
    }
    Ok(())
}

fn rvs_collect_config_key_paths_M(prefix: &str, item: &toml_edit::Item, out: &mut Vec<String>) {
    out.push(prefix.to_string());
    if let Some(table) = item.as_table_like() {
        for (key, child) in table.iter() {
            let child_prefix = format!("{prefix}.{key}");
            rvs_collect_config_key_paths_M(&child_prefix, child, out);
        }
    }
}

/// Unified callgraph collector.
///
/// Reserves private artifact and Cargo target directories, runs `cargo check`
/// with the callgraph collection environment, and returns the merged graph.
///
/// Dependency and std inference wrap every crate. Project analysis uses the
/// workspace-only collector below so third-party warnings stay capped.
///
/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_collect_callgraph_BIST(
    path: &Path,
    collection: CallgraphCollectionMode,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_BIST(path, collection, target_scope, vec![], local_crate_names)
}

pub(crate) fn rvs_collect_workspace_callgraph_BIST(
    path: &Path,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_BIST(
        path,
        CallgraphCollectionMode::Workspace,
        target_scope,
        vec![],
        local_crate_names,
    )
}

pub(crate) fn rvs_collect_callgraph_with_args_BIST(
    path: &Path,
    collection: CallgraphCollectionMode,
    target_scope: CargoTargetScope,
    extra_args: Vec<&str>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    rvs_collect_callgraph_with_args_detailed_BIST(
        path,
        collection,
        target_scope,
        extra_args,
        local_crate_names,
        CollectionLints::Silent,
    )
    .map(|collected| collected.callgraph)
    .map_err(|error| error.to_string())
}

fn rvs_collect_callgraph_with_args_detailed_BIST(
    path: &Path,
    collection: CallgraphCollectionMode,
    target_scope: CargoTargetScope,
    extra_args: Vec<&str>,
    local_crate_names: &BTreeSet<CrateName>,
    lints: CollectionLints,
) -> Result<CollectedCallgraph, CallgraphCollectionError> {
    let mut generation = rvs_reserve_run_generation_for_BIST(
        path,
        RunGenerationMode::Collection {
            collection: collection.into(),
            target_scope: target_scope.into(),
            lints,
        },
    )
    .map_err(|error| CallgraphCollectionError::Artifact(error.to_string()))?;
    let collection_result = (|| {
        let cargo_project = match collection {
            CallgraphCollectionMode::StandardLibrary => {
                rvs_create_std_probe_BIS(&generation).map_err(CallgraphCollectionError::Artifact)?
            }
            CallgraphCollectionMode::Workspace | CallgraphCollectionMode::AllCrates => {
                path.to_path_buf()
            }
        };
        rvs_run_cargo_check_impl_BIST(&CargoCheckConfig {
            project_path: &cargo_project,
            generation: &generation,
            mode: CargoCheckMode::Callgraph {
                collection,
                artifact_dir: generation.rvs_artifact_dir().to_path_buf(),
            },
            target_scope,
            extra_args,
            target_subdir: Some(generation.rvs_target_subdir()),
        })
        .map_err(CallgraphCollectionError::Cargo)?;
        let callgraph = rvs_merge_generation_callgraph_dir_BIS(
            generation.rvs_artifact_dir(),
            generation.rvs_generation_id(),
            local_crate_names,
        )
        .map_err(|error| CallgraphCollectionError::Artifact(error.to_string()))?;
        Ok(CollectedCallgraph { callgraph })
    })();
    let cleanup_result = rvs_cleanup_run_generation_BIMS(&mut generation);
    match (collection_result, cleanup_result) {
        (Ok(callgraph), Ok(())) => Ok(callgraph),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(CallgraphCollectionError::Artifact(cleanup)),
        (Err(error), Err(cleanup)) => {
            eprintln!("warning: additionally failed to clean callgraph generation: {cleanup}");
            Err(error)
        }
    }
}

fn rvs_create_std_probe_BIS(generation: &RivusRunGeneration) -> Result<PathBuf, String> {
    let probe = generation.rvs_root().join("std-probe");
    let source_dir = probe.join("src");
    std::fs::create_dir(&probe)
        .map_err(|error| format!("cannot create std probe {}: {error}", probe.display()))?;
    std::fs::create_dir(&source_dir).map_err(|error| {
        format!(
            "cannot create std probe source directory {}: {error}",
            source_dir.display()
        )
    })?;
    let manifest = probe.join("Cargo.toml");
    std::fs::write(&manifest, "[package]\nname = \"rivus-std-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[profile.dev]\ndebug-assertions = false\n")
        .map_err(|error| format!("cannot write std probe manifest {}: {error}", manifest.display()))?;
    let source = source_dir.join("main.rs");
    std::fs::write(&source, "fn main() {}\n").map_err(|error| {
        format!(
            "cannot write std probe source {}: {error}",
            source.display()
        )
    })?;
    Ok(probe)
}

fn rvs_reserve_run_generation_for_BIST(
    project_path: &Path,
    mode: RunGenerationMode,
) -> Result<RivusRunGeneration, RunGenerationError> {
    let project_path =
        project_path
            .canonicalize()
            .map_err(|source| RunGenerationError::CanonicalizeProject {
                path: project_path.to_path_buf(),
                source,
            })?;
    let lexical_runs_dir = project_path.join("target/.rivus-runs");
    std::fs::create_dir_all(&lexical_runs_dir).map_err(|source| {
        RunGenerationError::CreateDirectory {
            path: lexical_runs_dir.clone(),
            source,
        }
    })?;
    let runs_dir = lexical_runs_dir.canonicalize().map_err(|source| {
        RunGenerationError::CanonicalizeRunsDirectory {
            path: lexical_runs_dir,
            source,
        }
    })?;

    let prefix = format!("rivus-v4-{}-{}-", mode.rvs_name(), mode.rvs_target_name());
    let temp_dir = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(&runs_dir)
        .map_err(|source| RunGenerationError::CreateDirectory {
            path: runs_dir.clone(),
            source,
        })?;
    let root = temp_dir.path().to_path_buf();
    let generation_id = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();

    let artifact_dir = root.join("artifacts");
    std::fs::create_dir(&artifact_dir).map_err(|source| RunGenerationError::CreateDirectory {
        path: artifact_dir.clone(),
        source,
    })?;

    let marker = RunGenerationMarker {
        schema_version: RVS_RUN_GENERATION_SCHEMA_VERSION,
        generation_id: generation_id.clone(),
        project_root: project_path,
        mode,
    };
    rvs_mark_run_generation_ready_BIS(&root, &marker)?;

    let target_subdir = Path::new(".rivus-runs")
        .join(&generation_id)
        .join("cargo-target")
        .to_string_lossy()
        .into_owned();

    Ok(RivusRunGeneration {
        temp_dir: Some(temp_dir),
        root,
        artifact_dir,
        generation_id,
        target_subdir,
    })
}

#[cfg(test)]
fn rvs_test_generation_mode(purpose: &str) -> RunGenerationMode {
    match purpose {
        "lint" => RunGenerationMode::Analysis {
            target_scope: RunGenerationTargetScope::WithTestExampleBench,
            analysis: RunGenerationAnalysisMode::ProjectCaps,
        },
        "callgraph" => RunGenerationMode::Collection {
            collection: RunGenerationCollectionMode::Workspace,
            target_scope: RunGenerationTargetScope::Production,
            lints: CollectionLints::Silent,
        },
        "callgraph-std" => RunGenerationMode::Collection {
            collection: RunGenerationCollectionMode::StandardLibrary,
            target_scope: RunGenerationTargetScope::Production,
            lints: CollectionLints::Silent,
        },
        _ => RunGenerationMode::Analysis {
            target_scope: RunGenerationTargetScope::WithTestExampleBench,
            analysis: RunGenerationAnalysisMode::ProjectCaps,
        },
    }
}

#[cfg(test)]
fn rvs_reserve_run_generation_BIST(
    project_path: &Path,
    purpose: &str,
) -> Result<RivusRunGeneration, String> {
    rvs_reserve_run_generation_for_BIST(project_path, rvs_test_generation_mode(purpose))
        .map_err(|error| error.to_string())
}

fn rvs_mark_run_generation_ready_BIS(
    root: &Path,
    marker: &RunGenerationMarker,
) -> Result<(), RunGenerationError> {
    let path = root.join(RVS_RUN_GENERATION_MARKER_FILE);
    let json =
        serde_json::to_vec(marker).map_err(|source| RunGenerationError::SerializeMarker {
            path: path.clone(),
            source,
        })?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| RunGenerationError::CreateMarker {
            path: path.clone(),
            source,
        })?;
    file.write_all(&json)
        .map_err(|source| RunGenerationError::WriteMarker {
            path: path.clone(),
            source,
        })?;
    file.sync_all()
        .map_err(|source| RunGenerationError::SyncMarker { path, source })?;
    Ok(())
}

fn rvs_read_run_generation_marker_BIS(
    root: &Path,
) -> Result<RunGenerationMarker, RunGenerationError> {
    let path = root.join(RVS_RUN_GENERATION_MARKER_FILE);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(RunGenerationError::MarkerNotFile { path }),
        Err(source) => return Err(RunGenerationError::ReadMarker { path, source }),
    }
    let json = super::fs_guard::rvs_read_file_utf8_BIS(&path).map_err(|source| {
        RunGenerationError::ReadMarker {
            path: path.clone(),
            source,
        }
    })?;
    let marker: RunGenerationMarker =
        serde_json::from_str(&json).map_err(|source| RunGenerationError::ParseMarker {
            path: path.clone(),
            source,
        })?;
    if marker.schema_version != RVS_RUN_GENERATION_SCHEMA_VERSION {
        return Err(RunGenerationError::MarkerVersion {
            path,
            actual: marker.schema_version,
            expected: RVS_RUN_GENERATION_SCHEMA_VERSION,
        });
    }
    let dir_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if dir_name != marker.generation_id {
        return Err(RunGenerationError::MarkerIdentity { path: root.into() });
    }
    let project_path = marker.project_root.canonicalize().map_err(|source| {
        RunGenerationError::CanonicalizeProject {
            path: marker.project_root.clone(),
            source,
        }
    })?;
    if !marker.project_root.is_absolute() || marker.project_root != project_path {
        return Err(RunGenerationError::MarkerIdentity { path: root.into() });
    }
    Ok(marker)
}

fn rvs_cleanup_run_generation_BIMS(generation: &mut RivusRunGeneration) -> Result<(), String> {
    generation
        .rvs_cleanup_BIMS()
        .map_err(|error| error.to_string())
}

fn rvs_load_required_std_callgraph_cache_BIS(path: &Path) -> Result<FnGraph, String> {
    match rvs_load_published_std_callgraph_cache_BIS(path) {
        Ok(Some(cg)) => {
            if !cg
                .rvs_keys()
                .any(|path| rvs_is_std_like_def_path(path.rvs_as_str()))
            {
                return Err("published std callgraph cache contains no std-like functions; run cargo rivus infer-std first".into());
            }
            return Ok(cg);
        }
        Ok(None) => {}
        Err(error) => return Err(format!("{error}; run cargo rivus infer-std first")),
    }
    let cg_std_dir = super::callgraph_cache::rvs_std_callgraph_cache_dir(path);
    if super::fs_guard::rvs_validate_optional_dir_BIS(&cg_std_dir, "std callgraph cache")? {
        let cg = rvs_merge_callgraph_dir_BIS(&cg_std_dir, &BTreeSet::new())
            .map_err(|e| format!("{e}; run cargo rivus infer-std first"))?;
        let mut std_only = cg;
        rvs_filter_std_like_callgraph_M(&mut std_only, &BTreeSet::new());
        if !std_only.rvs_is_empty() {
            return Ok(std_only);
        }
    }
    Err("std callgraph cache not found; run cargo rivus infer-std first".into())
}

pub(crate) fn rvs_load_project_caps_BIS(path: &Path) -> Result<capsmap::CapsMap, String> {
    let caps_dir = path.join("caps");
    rvs_validate_optional_capsmap_dir_BIS(&caps_dir)?;
    CapsMap::rvs_load_effective_dir_BIS(&caps_dir).map_err(|e| format!("caps/: {e}"))
}

pub(crate) fn rvs_load_callgraph_and_caps_for_function_BIST(
    path: &Path,
    function: &str,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let callgraph = if rvs_should_use_required_std_cache(function, local_crate_names) {
        rvs_load_required_std_callgraph_cache_BIS(path)?
    } else {
        rvs_collect_project_callgraph_with_optional_std_cache_BIST(
            path,
            target_scope,
            local_crate_names,
        )?
    };
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_should_use_required_std_cache(
    function: &str,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    rvs_is_std_like_def_path(function)
        && !LocalScope::rvs_new(local_crate_names).rvs_contains_str(function)
}

pub(crate) fn rvs_collect_callgraph_and_caps_BIST(
    path: &Path,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<(FnGraph, capsmap::CapsMap), String> {
    let callgraph = rvs_collect_project_callgraph_with_optional_std_cache_BIST(
        path,
        target_scope,
        local_crate_names,
    )?;
    let caps = rvs_load_project_caps_BIS(path)?;
    Ok((callgraph, caps))
}

fn rvs_collect_project_callgraph_with_optional_std_cache_BIST(
    path: &Path,
    target_scope: CargoTargetScope,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, String> {
    let mut callgraph =
        rvs_collect_workspace_callgraph_BIST(path, target_scope, local_crate_names)?;
    match rvs_load_published_std_callgraph_cache_BIS(path) {
        Ok(Some(std_graph)) => {
            rvs_merge_std_like_callgraph_with_local_prefixes_M(
                &mut callgraph,
                &std_graph,
                local_crate_names,
            )
            .map_err(|error| format!("cannot merge published std callgraph cache: {error}"))?;
            return Ok(callgraph);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("warning: ignoring stale published std callgraph cache: {error}");
            return Ok(callgraph);
        }
    }
    let cg_std_dir = super::callgraph_cache::rvs_std_callgraph_cache_dir(path);
    if rvs_warn_optional_dir_BIST(&cg_std_dir, "std callgraph cache") {
        match rvs_merge_callgraph_dir_BIS(&cg_std_dir, &BTreeSet::new()) {
            Ok(std_graph) => {
                if let Err(error) = rvs_merge_std_like_callgraph_with_local_prefixes_M(
                    &mut callgraph,
                    &std_graph,
                    local_crate_names,
                ) {
                    eprintln!("warning: ignoring incompatible legacy std callgraph cache: {error}");
                }
            }
            Err(e) => eprintln!("warning: ignoring stale std callgraph cache: {e}"),
        }
    }
    Ok(callgraph)
}

fn rvs_warn_optional_dir_BIST(path: &Path, label: &str) -> bool {
    match super::fs_guard::rvs_validate_optional_dir_BIS(path, label) {
        Ok(exists) => exists,
        Err(e) => {
            eprintln!("warning: ignoring stale {label}: {e}");
            false
        }
    }
}

#[cfg(test)]
pub(crate) fn rvs_write_capsmap_result_BIST(
    result: &str,
    output: &Path,
    label: &str,
) -> Result<(), String> {
    rvs_write_capsmap_file_BIST(output, result, label)?;
    println!("Written {label} to {}", output.display());
    Ok(())
}

pub(crate) fn rvs_write_pinned_capsmap_result_BIST(
    result: &str,
    publication: &Path,
    label: &str,
) -> Result<(), String> {
    super::fs_guard::rvs_atomic_write_BIST(publication, result.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", publication.display()))?;
    println!("Written {label} to {}", publication.display());
    Ok(())
}

pub(crate) fn rvs_preflight_capsmap_file_BIS(path: &Path, label: &str) -> Result<(), String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let separator = std::path::MAIN_SEPARATOR as u8;
    let mut end = bytes.len();
    while end > 0 && bytes.get(end - 1).is_some_and(|byte| *byte == separator) {
        end -= 1;
    }
    let trimmed = bytes.get(..end).unwrap_or(bytes);
    let has_trailing_separator = end < bytes.len();
    let ends_in_dot_component = trimmed == b"."
        || trimmed == b".."
        || trimmed.ends_with(&[separator, b'.'])
        || trimmed.ends_with(&[separator, b'.', b'.']);
    if has_trailing_separator
        || ends_in_dot_component
        || !matches!(path.components().next_back(), Some(std::path::Component::Normal(name)) if !name.is_empty())
    {
        return Err(format!(
            "{label} output path must name a file: {}",
            path.display()
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{label} output path must not contain '..': {}",
            path.display()
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "{label} output must be a regular file, not a symlink: {}",
            path.display()
        )),
        Ok(metadata) if metadata.is_dir() => {
            Err(format!("{label} output must be a file: {}", path.display()))
        }
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "{label} output must be a regular file: {}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "cannot inspect {label} output {}: {e}",
            path.display()
        )),
    }?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        super::fs_guard::rvs_validate_optional_dir_BIS(parent, &format!("{label} output parent"))?;
    }
    Ok(())
}

#[cfg(test)]
fn rvs_write_capsmap_file_BIST(path: &Path, result: &str, label: &str) -> Result<(), String> {
    rvs_preflight_capsmap_file_BIS(path, label)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create parent for {}: {e}", path.display()))?;
    }
    super::fs_guard::rvs_atomic_write_BIST(path, result.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

pub(crate) fn rvs_load_local_crate_prefixes_BIS(
    path: &Path,
    target_scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_ensure_cargo_project_BIS(path)?;
    rvs_detect_local_crate_prefixes_BIS(path, target_scope)
}

pub(crate) fn rvs_clean_dir_BIS(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path)
            .map_err(|e| format!("cannot remove {}: {e}", path.display()))?,
        Ok(_) => std::fs::remove_file(path)
            .map_err(|e| format!("cannot remove {}: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot inspect {}: {e}", path.display())),
    }
    Ok(())
}

/// Validate that `path` is a directory, returning an error message if not.
pub(crate) fn rvs_ensure_project_dir_BIS(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }
    Ok(())
}

pub(crate) fn rvs_ensure_cargo_project_BIS(path: &Path) -> Result<(), String> {
    rvs_ensure_project_dir_BIS(path)?;
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Err(format!("'{}' is not a Cargo project", path.display()));
    }
    Ok(())
}

pub(crate) fn rvs_canonical_cargo_project_BIS(path: &Path) -> Result<PathBuf, String> {
    rvs_ensure_cargo_project_BIS(path)?;
    path.canonicalize()
        .map_err(|error| format!("cannot canonicalize '{}': {error}", path.display()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallEdgeType, FunctionIdentity};
    use crate::test_support::{
        rvs_caps_v2, rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };
    use std::collections::BTreeMap;

    fn rvs_make_workspace_temp_dir_BIS(tag: &str) -> PathBuf {
        rvs_make_temp_dir_BIS(&format!("workspace-{tag}"))
    }

    fn rvs_reserve_cargo_check_test_generation_BIST(
        project: &Path,
        mode: &CargoCheckMode,
        target_scope: CargoTargetScope,
    ) -> RivusRunGeneration {
        let mode = match mode {
            CargoCheckMode::Lint(CargoLintInput::Offline(_)) => RunGenerationMode::Analysis {
                target_scope: target_scope.into(),
                analysis: RunGenerationAnalysisMode::Offline,
            },
            CargoCheckMode::Callgraph { collection, .. } => RunGenerationMode::Collection {
                collection: (*collection).into(),
                target_scope: target_scope.into(),
                lints: CollectionLints::Silent,
            },
        };
        rvs_reserve_run_generation_for_BIST(project, mode)
            .expect("never: cargo command test generation should be reserved")
    }

    fn rvs_targeted_test_node(crate_id: u64) -> crate::artifacts::FnNode {
        debug_assert!(crate_id > 0, "test target crate id is nonzero");
        let mut node = crate::artifacts::FnNode::default();
        node.crate_id = crate_id;
        node.is_production = true;
        node.is_coverage_candidate = true;
        node.crate_provenance = crate::artifacts::CrateProvenance::Dependency;
        node
    }

    fn rvs_support_inference_test_graph() -> FnGraph {
        let support_path = crate::symbols::DefPath::from("support_crate::help");
        let mut std_node = rvs_targeted_test_node(1);
        let support_identity = crate::artifacts::FunctionIdentity {
            crate_id: 2,
            def_path: support_path.clone(),
        };
        std_node
            .calls
            .insert(support_identity.clone(), CallEdgeType::Strong);
        std_node
            .call_sites
            .insert(crate::artifacts::CallSiteIdentity {
                callee: support_identity,
                occurrence: 0,
                source: None,
            });
        let mut support_node = rvs_targeted_test_node(2);
        let boundary_path = crate::symbols::DefPath::from("ffi_support::rvs_read_BI");
        let boundary_identity = crate::artifacts::FunctionIdentity {
            crate_id: 3,
            def_path: boundary_path.clone(),
        };
        support_node
            .calls
            .insert(boundary_identity.clone(), CallEdgeType::Strong);
        support_node
            .calls
            .insert(boundary_identity.clone(), CallEdgeType::Strong);
        support_node
            .call_sites
            .insert(crate::artifacts::CallSiteIdentity {
                callee: boundary_identity,
                occurrence: 0,
                source: None,
            });
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            crate::symbols::DefPath::from("std::fs::read_to_string"),
            std_node,
        );
        graph.rvs_insert_M(support_path, support_node);
        graph
    }

    fn rvs_command_env_value(cmd: &Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs().find_map(|(name, value)| {
            if name == key {
                Some(value.map(|v| v.to_string_lossy().into_owned()))
            } else {
                None
            }
        })
    }

    #[test]
    fn test_20260630_collect_local_crate_prefixes_bin_name() {
        let input = "[package]\nname = \"rivus-linter\"\n\n[[bin]]\nname = \"cargo-rivus\"\npath = \"src/main.rs\"\n";
        let prefixes = rvs_collect_local_crate_prefixes(input).expect("prefixes should parse");
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260630_collect_local_crate_prefixes_bin_name",
            &output,
        );
        assert!(prefixes.contains("rivus_linter"));
        assert!(prefixes.contains("cargo_rivus"));
    }

    #[test]
    fn test_20260702_collect_local_crate_prefixes_rejects_workspace_root() {
        let input = "[workspace]\nmembers = []\nresolver = \"2\"\n";
        let result = rvs_collect_local_crate_prefixes(input);
        let output = format!("{result:?}");
        rvs_snapshot_BIS(
            "test_20260702_collect_local_crate_prefixes_rejects_workspace_root",
            &output,
        );
        assert!(result.is_err());
        assert!(output.contains("missing local crate target"));
    }

    #[test]
    fn test_20260710_callgraph_source_records_exact_workspace_member_base() {
        let workspace = rvs_make_workspace_temp_dir_BIS("source-provenance");
        let member = workspace.join("member");
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let member_text = "pub fn rvs_parse() -> u8 { 1 }\n";
        std::fs::write(member.join("src/lib.rs"), member_text).unwrap();

        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "pub fn workspace_decoy() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(member.join("member/src")).unwrap();
        std::fs::write(
            member.join("member/src/lib.rs"),
            "pub fn nested_decoy() {}\n",
        )
        .unwrap();

        let graph = rvs_collect_callgraph_BIST(
            &member,
            CallgraphCollectionMode::AllCrates,
            CargoTargetScope::Production,
            &BTreeSet::from([CrateName::from("member")]),
        )
        .unwrap();
        let source = graph
            .rvs_get("member::rvs_parse")
            .and_then(|node| node.sources.first())
            .expect("member function should have source metadata");
        let normalized =
            crate::environment::rename::rvs_normalize_source_for_project_BIS(source, &member)
                .unwrap();
        let range = source.name_start as usize..source.name_end as usize;
        let recorded_name = member_text
            .get(range)
            .expect("source range should select the function name");
        let output = format!(
            "file={}\nbase={}\nrelative={}\nnormalized={}\nname={recorded_name}\n",
            source.file.display(),
            source
                .base
                .as_deref()
                .map_or("<none>".into(), |base| base.display().to_string()),
            source.file.is_relative(),
            normalized.file.display(),
        )
        .replace(&workspace.to_string_lossy().into_owned(), "$WORKSPACE");
        rvs_snapshot_BIS(
            "test_20260710_callgraph_source_records_exact_workspace_member_base",
            &output,
        );

        if source.file.is_relative() {
            assert!(source.base.is_some());
        }
        assert_eq!(
            normalized.file,
            member.join("src/lib.rs").canonicalize().unwrap()
        );
        assert_eq!(recorded_name, "rvs_parse");
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn test_20260714_check_accepts_const_only_crate() {
        let dir = rvs_make_workspace_temp_dir_BIS("const-only-check");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"const-only\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautobins = false\nautotests = false\nautoexamples = false\nautobenches = false\n\n[lib]\ntest = false\nbench = false\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub const ANSWER: u8 = 42;\n").unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nmissing_artifact={}\n",
            output.status.success(),
            stderr.contains("no callgraph JSON artifacts") || stderr.contains("contained no nodes")
        );
        rvs_snapshot_BIS("test_20260714_check_accepts_const_only_crate", &summary);

        assert!(output.status.success(), "{stderr}");
        assert!(!summary.contains("missing_artifact=true"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_project_callgraph_excludes_path_dependency_nodes() {
        let dir = rvs_make_workspace_temp_dir_BIS("project-callgraph-local-only");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn rvs_local() { fixture_dep::dependency_helper(); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "pub fn dependency_helper() {}\n",
        )
        .unwrap();

        let local = BTreeSet::from([CrateName::from("local-app")]);
        let graph = rvs_collect_project_callgraph_with_optional_std_cache_BIST(
            &dir,
            CargoTargetScope::Production,
            &local,
        )
        .unwrap();
        let paths = graph
            .rvs_keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let output = paths.join("\n") + "\n";
        rvs_snapshot_BIS(
            "test_20260714_project_callgraph_excludes_path_dependency_nodes",
            &output,
        );

        assert!(paths.iter().any(|path| path == "local_app::rvs_local"));
        assert!(!paths.iter().any(|path| path.starts_with("fixture_dep::")));
        assert!(graph.rvs_get("local_app::rvs_local").is_some_and(|node| {
            node.calls.keys().any(|identity| {
                identity.def_path == crate::symbols::DefPath::from("fixture_dep::dependency_helper")
            })
        }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_offline_caps_emission_respects_cfg_target_identity() {
        let dir = rvs_make_workspace_temp_dir_BIS("offline-caps-cfg-target-identity");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"offline-cfg\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![allow(rivus::rvs_untested_good_fn)]\n#![cfg_attr(not(test), allow(rivus::rvs_contract_mismatch))]\n#![cfg_attr(test, deny(rivus::rvs_contract_mismatch))]\n\nstatic VALUE: u8 = 1;\nfn rvs_effect_S() -> u8 { VALUE }\n\n#[cfg(not(test))]\npub fn rvs_variant() -> u8 { rvs_effect_S() }\n\n#[cfg(test)]\npub fn rvs_variant() -> u8 { 0 }\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nmissing_side_effect={}\nunmatched_emission={}\n",
            output.status.success(),
            stderr.contains("is missing capability marker missing_side_effect"),
            stderr.contains("diagnostic was not matched by the final compilation"),
        );
        rvs_snapshot_BIS(
            "test_20260715_offline_caps_emission_respects_cfg_target_identity",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260716_shared_def_path_entrypoint_is_target_scoped() {
        let dir = rvs_make_workspace_temp_dir_BIS("shared-def-path-entrypoint");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"shared-entrypoint\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![allow(rivus::rvs_non_rvs_fn)]\n#![allow(rivus::rvs_untested_good_fn)]\n\npub fn main() {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![allow(rivus::rvs_untested_good_fn)]\n#![deny(rivus::rvs_non_rvs_fn)]\n\nstatic VALUE: u8 = 1;\nfn rvs_effect_S() -> u8 { VALUE }\nfn main() { let _ = rvs_effect_S(); }\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nmissing_prefix={}\nunmatched_emission={}\n",
            output.status.success(),
            stderr.contains("'main' is missing the rvs_ prefix"),
            stderr.contains("diagnostic was not matched by the final compilation"),
        );
        rvs_snapshot_BIS(
            "test_20260716_shared_def_path_entrypoint_is_target_scoped",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        assert!(!stderr.contains("'main' is missing the rvs_ prefix"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260716_cfg_gated_call_does_not_emit_in_absent_test_variant() {
        let dir = rvs_make_workspace_temp_dir_BIS("offline-call-cfg-statement");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"offline-call-cfg-statement\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(non_snake_case)]\n#![allow(rivus::rvs_untested_good_fn)]\n#![cfg_attr(not(test), allow(rivus::rvs_contract_mismatch))]\n#![cfg_attr(test, deny(rivus::rvs_contract_mismatch))]\n\nstatic VALUE: u8 = 1;\nfn rvs_effect_S() -> u8 { VALUE }\npub fn rvs_variant() -> u8 {\n    #[cfg(not(test))]\n    { return rvs_effect_S(); }\n    #[cfg(test)]\n    { 0 }\n}\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = format!(
            "success={}\nmissing_side_effect={}\nunmatched_emission={}\n",
            output.status.success(),
            stderr.contains("is missing capability marker missing_side_effect"),
            stderr.contains("diagnostic was not matched by the final compilation"),
        );
        rvs_snapshot_BIS(
            "test_20260716_cfg_gated_call_does_not_emit_in_absent_test_variant",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_offline_emission_acknowledgements_are_required() {
        let generation = rvs_make_workspace_temp_dir_BIS("offline-emission-ack");
        let ack_dir = generation.join("offline-emission-acks");
        std::fs::create_dir(&ack_dir).unwrap();
        let emissions = vec![crate::offline_caps::OfflineCapsEmission {
            lint: crate::offline_caps::OfflineCapsLint::DuplicateSuffix,
            span_anchors: BTreeSet::from([crate::offline_caps::OfflineCapsEmissionAnchor {
                identity: crate::artifacts::FunctionIdentity {
                    crate_id: 7,
                    def_path: crate::symbols::DefPath::from("demo::rvs_call"),
                },
                call_site: None,
            }]),
            message: "violation".to_string(),
        }];

        let missing = rvs_verify_offline_emission_acks_BIS(&generation, &emissions);
        std::fs::write(
            ack_dir.join(crate::offline_caps::rvs_emission_ack_name(0, 0)),
            [],
        )
        .unwrap();
        let present = rvs_verify_offline_emission_acks_BIS(&generation, &emissions);
        let output = format!(
            "missing_error={}\npresent_ok={}\n",
            missing.is_err(),
            present.is_ok()
        );
        rvs_snapshot_BIS(
            "test_20260715_offline_emission_acknowledgements_are_required",
            &output,
        );

        assert!(missing.is_err());
        assert_eq!(present, Ok(VerifiedOfflineEmissionAcks));
        std::fs::remove_dir_all(generation).unwrap();
    }

    #[test]
    fn test_20260714_callgraph_collection_caps_source_deny_warnings() {
        let dir = rvs_make_workspace_temp_dir_BIS("collection-deny-warnings");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"collection-deny-warnings\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![deny(warnings)]\n#![allow(non_snake_case)]\n\npub fn rvs_value() -> i32 {\n    let unused = 1;\n    42\n}\n",
        )
        .unwrap();
        let local = BTreeSet::from([CrateName::from("collection-deny-warnings")]);

        let result =
            rvs_collect_workspace_callgraph_BIST(&dir, CargoTargetScope::Production, &local);
        let graph_has_function = result.as_ref().is_ok_and(|graph| {
            graph
                .rvs_get("collection_deny_warnings::rvs_value")
                .is_some()
        });
        let output = format!(
            "result_ok={}\ngraph_has_function={graph_has_function}\n",
            result.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_collection_caps_source_deny_warnings",
            &output,
        );

        assert!(result.is_ok(), "{result:?}");
        assert!(graph_has_function);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_merged_coverage_conservatively_merges_same_path_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("merged-coverage-target-identity");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"same-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![allow(non_snake_case)]\n\npub fn rvs_same() -> i32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_20260714_calls_library_same() {\n        assert_eq!(super::rvs_same(), 1);\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "#![allow(non_snake_case)]\n\n#[allow(dead_code)]\nfn rvs_bin_value() -> i32 { 1 }\n\npub fn rvs_same() -> i32 { rvs_bin_value() }\n\nfn main() {\n    let _ = rvs_same();\n}\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let has_merge_conflict =
            stderr.contains("conflicting ordinary definitions across Cargo targets");
        let report_output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("report")
            .arg(&dir)
            .output()
            .unwrap();
        let report_stdout = String::from_utf8_lossy(&report_output.stdout);
        let report_counts_both = report_stdout.contains("Total: 1 functions, 1 lines");
        let summary = format!(
            "success={}\nmerge_conflict={has_merge_conflict}\nreport_success={}\nreport_counts_both={report_counts_both}\n",
            output.status.success(),
            report_output.status.success(),
        );
        rvs_snapshot_BIS(
            "test_20260714_merged_coverage_conservatively_merges_same_path_targets",
            &summary,
        );

        assert!(output.status.success());
        assert!(!has_merge_conflict);
        assert!(report_output.status.success(), "{report_stdout}");
        assert!(report_counts_both, "{report_stdout}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_unresolved_local_callable_does_not_cover_function() {
        let dir = rvs_make_workspace_temp_dir_BIS("coverage-local-callable");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-callable\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n#![allow(internal_features)]\n#![allow(non_snake_case)]\n#![warn(rivus::rvs_untested_good_fn)]\n\n/// Production function intentionally left uncovered.\npub fn rvs_target() -> i32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_20260714_calls_local_callable() {\n        let rvs_target = || 2;\n        assert_eq!(rvs_target(), 2);\n    }\n}\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warning_count = stderr
            .matches("good fn 'rvs_target' not called by any test")
            .count();
        let summary = format!(
            "success={}\nwarning_count={warning_count}\n",
            output.status.success(),
        );
        rvs_snapshot_BIS(
            "test_20260714_unresolved_local_callable_does_not_cover_function",
            &summary,
        );

        assert!(output.status.success(), "{stderr}");
        assert_eq!(warning_count, 1, "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260709_collect_local_crate_prefixes_rejects_invalid_target_names_table() {
        let cases = [
            (
                "empty",
                "[package]\nname = \"\"\n",
                "[[bin]]\nname = \"\"\n",
                "",
            ),
            (
                "whitespace",
                "[package]\nname = \"bad name\"\n",
                "[lib]\nname = \" bad\"\n",
                "bad\tname",
            ),
            (
                "pathy",
                "[package]\nname = \"bad/name\"\n",
                "[lib]\nname = \"bad\\\\name\"\n",
                "bad\0name",
            ),
        ];
        let mut output = String::new();
        for (name, first_input, second_input, helper_name) in cases {
            let first = rvs_collect_local_crate_prefixes(first_input);
            let second = rvs_collect_local_crate_prefixes(second_input);
            let mut prefixes = BTreeSet::new();
            let helper = rvs_insert_manifest_crate_name_M(&mut prefixes, "demo", helper_name);
            output.push_str(&format!(
                "{name}: first={first:?} second={second:?} helper={helper:?} len={}\n",
                prefixes.len()
            ));
            assert!(first.is_err(), "{name}");
            assert!(second.is_err(), "{name}");
            assert!(helper.is_err(), "{name}");
            assert!(prefixes.is_empty(), "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260709_collect_local_crate_prefixes_rejects_invalid_target_names_table",
            &output,
        );
    }

    #[test]
    fn test_20260705_collect_local_crate_prefixes_test_example_bench() {
        let input = r#"
[[test]]
name = "integration-test"

[[example]]
name = "demo-example"

[[bench]]
name = "throughput-bench"
"#;
        let prefixes = rvs_collect_local_crate_prefixes(input).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_collect_local_crate_prefixes_test_example_bench",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("integration_test")));
        assert!(prefixes.contains(&CrateName::from("demo_example")));
        assert!(prefixes.contains(&CrateName::from("throughput_bench")));
    }

    #[test]
    fn test_20260706_collect_prefixes_for_build_targets() {
        let input = r#"
[package]
name = "core-demo"

[[bin]]
name = "tool-bin"

[[test]]
name = "integration-test"

[[example]]
name = "demo-example"

[[bench]]
name = "throughput-bench"
"#;
        let prefixes =
            rvs_collect_local_crate_prefixes_for_targets(input, CargoTargetScope::Production)
                .unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS("test_20260706_collect_prefixes_for_build_targets", &output);

        assert!(prefixes.contains(&CrateName::from("core_demo")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(!prefixes.contains(&CrateName::from("integration_test")));
        assert!(!prefixes.contains(&CrateName::from("demo_example")));
        assert!(!prefixes.contains(&CrateName::from("throughput_bench")));
    }

    #[test]
    fn test_20260712_cargo_target_scope_selects_optional_targets() {
        let input = r#"
[package]
name = "core-demo"

[[bin]]
name = "tool-bin"

[[test]]
name = "integration-test"

[[example]]
name = "demo-example"

[[bench]]
name = "throughput-bench"
"#;
        let production =
            rvs_collect_local_crate_prefixes_for_targets(input, CargoTargetScope::Production)
                .expect("production targets should parse");
        let with_optional = rvs_collect_local_crate_prefixes_for_targets(
            input,
            CargoTargetScope::WithTestExampleBench,
        )
        .expect("all targets should parse");
        let rvs_render = |targets: &BTreeSet<CrateName>| {
            targets
                .iter()
                .map(CrateName::rvs_as_str)
                .collect::<Vec<_>>()
                .join(",")
        };
        let output = format!(
            "production={}\nwith_optional={}\n",
            rvs_render(&production),
            rvs_render(&with_optional),
        );
        rvs_snapshot_BIS(
            "test_20260712_cargo_target_scope_selects_optional_targets",
            &output,
        );

        assert_eq!(
            production,
            BTreeSet::from([CrateName::from("core_demo"), CrateName::from("tool_bin")])
        );
        assert_eq!(
            with_optional,
            BTreeSet::from([
                CrateName::from("core_demo"),
                CrateName::from("demo_example"),
                CrateName::from("integration_test"),
                CrateName::from("throughput_bench"),
                CrateName::from("tool_bin"),
            ])
        );
    }

    #[test]
    fn test_20260705_detect_local_crate_prefixes_auto_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-prefixes");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"auto-target-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo-example.rs"),
            ("benches", "fast-bench.rs"),
            ("src/bin", "tool-bin.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }
        std::fs::create_dir_all(dir.join("src/bin/server")).unwrap();
        std::fs::write(dir.join("src/bin/server/main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(dir.join("examples/nested-example")).unwrap();
        std::fs::write(
            dir.join("examples/nested-example/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests/support")).unwrap();

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_detect_local_crate_prefixes_auto_targets",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("ui_tests")));
        assert!(prefixes.contains(&CrateName::from("demo_example")));
        assert!(prefixes.contains(&CrateName::from("fast_bench")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(prefixes.contains(&CrateName::from("server")));
        assert!(prefixes.contains(&CrateName::from("nested_example")));
        assert!(!prefixes.contains(&CrateName::from("support")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260705_detect_local_crate_prefixes_respects_auto_flags() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-prefix-flags");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"auto-flag-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautobins = false\nautotests = false\nautoexamples = false\nautobenches = false\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo.rs"),
            ("benches", "bench.rs"),
            ("src/bin", "tool.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260705_detect_local_crate_prefixes_respects_auto_flags",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("auto_flag_demo")));
        assert!(!prefixes.contains(&CrateName::from("ui_tests")));
        assert!(!prefixes.contains(&CrateName::from("demo")));
        assert!(!prefixes.contains(&CrateName::from("bench")));
        assert!(!prefixes.contains(&CrateName::from("tool")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_detect_local_crate_prefixes_build_script_modes() {
        let root = rvs_make_workspace_temp_dir_BIS("build-script-prefix-modes");
        let cases = [
            (
                "default",
                "[package]\nname = \"default-build\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                Some("build.rs"),
            ),
            (
                "absent",
                "[package]\nname = \"absent-build\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                None,
            ),
            (
                "explicit",
                "[package]\nname = \"explicit-build\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build/custom.rs\"\n",
                Some("build/custom.rs"),
            ),
            (
                "disabled",
                "[package]\nname = \"disabled-build\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = false\n",
                Some("build.rs"),
            ),
        ];
        let mut output = String::new();
        for (label, manifest, build_path) in cases {
            let dir = root.join(label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
            if let Some(build_path) = build_path {
                let build_path = dir.join(build_path);
                std::fs::create_dir_all(
                    build_path
                        .parent()
                        .expect("never: build script path has a parent"),
                )
                .unwrap();
                std::fs::write(build_path, "fn main() {}\n").unwrap();
            }

            let prefixes =
                rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
            let rendered = prefixes
                .iter()
                .map(CrateName::rvs_as_str)
                .collect::<Vec<_>>()
                .join(",");
            output.push_str(&format!("{label}={rendered}\n"));
            assert!(
                !prefixes.contains(&CrateName::from("build_script_build")),
                "build-script crates are excluded from local prefixes: {label}"
            );
        }
        rvs_snapshot_BIS(
            "test_20260715_detect_local_crate_prefixes_build_script_modes",
            &output,
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_20260715_build_script_functions_are_excluded() {
        let dir = rvs_make_cargo_project_BIS(
            "build-script-shared-scope",
            "build-script-shared-scope",
            &[
                ("src/lib.rs", "pub fn rvs_library() {}\n"),
                (
                    "build.rs",
                    "#![allow(non_snake_case)]\nfn rvs_build_helper() {}\nfn main() { rvs_build_helper(); }\n",
                ),
            ],
        );
        let local_crate_names =
            rvs_load_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
        let callgraph = rvs_collect_workspace_callgraph_BIST(
            &dir,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let path = crate::symbols::DefPath::from("build_script_build::rvs_build_helper");
        let output = format!(
            "prefix={}\nartifact={}\n",
            local_crate_names.contains(&CrateName::from("build_script_build")),
            callgraph.rvs_get(path.rvs_as_str()).is_some(),
        );
        rvs_snapshot_BIS("test_20260715_build_script_functions_are_excluded", &output);

        assert!(!local_crate_names.contains(&CrateName::from("build_script_build")));
        assert!(
            callgraph.rvs_get(path.rvs_as_str()).is_none(),
            "build-script helper must not enter the callgraph"
        );
        assert!(callgraph.rvs_get("build_script_build::main").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260814_ordinary_package_named_build_script_build_is_analyzed() {
        let dir = rvs_make_cargo_project_BIS(
            "ordinary-build-script-build",
            "build-script-build",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\npub fn rvs_value() -> u8 { 1 }\n",
            )],
        );
        let local_crate_names =
            rvs_load_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
        let callgraph = rvs_collect_workspace_callgraph_BIST(
            &dir,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let output = format!(
            "prefix={}\nnode={}\n",
            local_crate_names.contains(&CrateName::from("build_script_build")),
            callgraph.rvs_get("build_script_build::rvs_value").is_some(),
        );
        rvs_snapshot_BIS(
            "test_20260814_ordinary_package_named_build_script_build_is_analyzed",
            &output,
        );

        assert!(local_crate_names.contains(&CrateName::from("build_script_build")));
        assert!(
            callgraph.rvs_get("build_script_build::rvs_value").is_some(),
            "an ordinary package named build-script-build must be analyzed, not excluded"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_all_crates_excludes_build_script_nodes() {
        let dir = rvs_make_workspace_temp_dir_BIS("build-script-package-identity");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![allow(non_snake_case)]\npub fn rvs_local() -> u8 { fixture_dep::value() }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("build.rs"),
            "#![allow(non_snake_case)]\nfn rvs_shared_helper() {}\nfn main() { rvs_shared_helper(); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/build.rs"),
            "#![allow(non_snake_case)]\nfn rvs_shared_helper() {}\nfn main() { rvs_shared_helper(); }\n",
        )
        .unwrap();

        let local_crate_names =
            rvs_load_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
        let graph = rvs_collect_callgraph_BIST(
            &dir,
            CallgraphCollectionMode::AllCrates,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let build_script_nodes: Vec<String> = graph
            .rvs_keys()
            .filter(|path| path.rvs_is_build_script_crate())
            .map(|path| path.rvs_as_str().to_string())
            .collect();
        let output = format!("build_script_nodes={build_script_nodes:?}\n");
        rvs_snapshot_BIS(
            "test_20260729_all_crates_excludes_build_script_nodes",
            &output,
        );

        assert!(
            build_script_nodes.is_empty(),
            "local and dependency build scripts must both be excluded from the graph"
        );
        assert!(graph.rvs_get("local_app::rvs_local").is_some());
        assert!(graph.rvs_get("fixture_dep::value").is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260730_cargo_env_cannot_forge_primary_package_provenance() {
        let dir = rvs_make_workspace_temp_dir_BIS("cargo-env-primary-provenance");
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join(".cargo/config.toml"),
            "[env]\nCARGO_PRIMARY_PACKAGE = { value = \"1\", force = true }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn rvs_local() -> u8 { fixture_dep::rvs_dependency() }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "pub fn rvs_dependency() -> u8 { 1 }\n",
        )
        .unwrap();

        let graph = rvs_collect_callgraph_BIST(
            &dir,
            CallgraphCollectionMode::AllCrates,
            CargoTargetScope::Production,
            &BTreeSet::from([CrateName::from("local-app")]),
        )
        .unwrap();
        let dependency = graph
            .rvs_get("fixture_dep::rvs_dependency")
            .expect("never: path dependency function should be collected");
        let provenances = vec![dependency.crate_provenance];
        let output = format!("dependency_provenance={provenances:?}\n");
        rvs_snapshot_BIS(
            "test_20260730_cargo_env_cannot_forge_primary_package_provenance",
            &output,
        );

        assert_eq!(
            provenances,
            vec![crate::artifacts::CrateProvenance::Dependency]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_reports_invalid_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"demo\"\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_reports_invalid_cargo_toml",
            &format!("{result:?}\nlen={}\n", prefixes.len())
                .replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_rejects_file_target_dir() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-file-dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"file-dir-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("tests"), "not a directory\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_rejects_file_target_dir",
            &format!("{result:?}\nlen={}\n", prefixes.len())
                .replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_collect_auto_target_prefixes_ignores_rs_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-rs-directory");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"rs-dir-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests/fake.rs")).unwrap();
        std::fs::write(dir.join("tests/real.rs"), "fn main() {}\n").unwrap();
        let mut prefixes = BTreeSet::new();

        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_collect_auto_target_prefixes_ignores_rs_directory",
            &format!("result={result:?}\n{output}\n"),
        );

        assert!(result.is_ok());
        assert!(prefixes.contains(&CrateName::from("real")));
        assert!(!prefixes.contains(&CrateName::from("fake")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260709_collect_auto_target_prefixes_reject_invalid_names_table() {
        use std::os::unix::ffi::OsStringExt as _;

        let cases = [
            ("whitespace_rs_stem", "bad name.rs", None),
            (
                "non_utf8_rs_stem",
                "",
                Some(std::ffi::OsString::from_vec(vec![
                    b'b', b'a', b'd', 0xff, b'.', b'r', b's',
                ])),
            ),
            (
                "non_utf8_dir_name",
                "",
                Some(std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff])),
            ),
        ];
        let mut output = String::new();
        for (name, file_name, os_name) in cases {
            let dir = rvs_make_workspace_temp_dir_BIS(&format!("auto-target-{name}"));
            std::fs::write(
                dir.join("Cargo.toml"),
                "[package]\nname = \"invalid-target-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            std::fs::create_dir_all(dir.join("tests")).unwrap();
            match (name, os_name) {
                ("whitespace_rs_stem", _) => {
                    std::fs::write(dir.join("tests").join(file_name), "fn main() {}\n").unwrap();
                }
                ("non_utf8_rs_stem", Some(file_name)) => {
                    std::fs::write(dir.join("tests").join(file_name), "fn main() {}\n").unwrap();
                }
                ("non_utf8_dir_name", Some(dir_name)) => {
                    let target_dir = dir.join("tests").join(dir_name);
                    std::fs::create_dir_all(&target_dir).unwrap();
                    std::fs::write(target_dir.join("main.rs"), "fn main() {}\n").unwrap();
                }
                _ => unreachable!("case setup should be exhaustive"),
            }
            let mut prefixes = BTreeSet::new();
            let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
            output.push_str(&format!(
                "{name}: is_err={} len={}\n",
                result.is_err(),
                prefixes.len()
            ));
            assert!(result.is_err(), "{name}");
            assert!(prefixes.is_empty(), "{name}");
            std::fs::remove_dir_all(dir).unwrap();
        }
        rvs_snapshot_BIS(
            "test_20260709_collect_auto_target_prefixes_reject_invalid_names_table",
            &output,
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_collect_auto_target_prefixes_reports_first_sorted_error() {
        use std::os::unix::ffi::OsStringExt as _;

        let dir = rvs_make_workspace_temp_dir_BIS("auto-target-sorted-error");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"sorted-error-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/z bad.rs"), "fn main() {}\n").unwrap();
        let non_utf8_name = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'r', b's']);
        std::fs::write(dir.join("tests").join(non_utf8_name), "fn main() {}\n").unwrap();

        let mut prefixes = BTreeSet::new();
        let result = rvs_collect_auto_target_prefixes_BIMS(&dir, &mut prefixes);
        let output = format!("{result:?}\nlen={}\n", prefixes.len())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260714_collect_auto_target_prefixes_reports_first_sorted_error",
            &output,
        );

        assert!(output.contains("not UTF-8"), "{output}");
        assert!(prefixes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_detect_local_crate_prefixes_for_cargo_check_excludes_unchecked_targets() {
        let dir = rvs_make_workspace_temp_dir_BIS("production-target-prefixes");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"prod-prefix-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[[test]]\nname = \"tokio\"\n\n[[example]]\nname = \"serde\"\n\n[[bench]]\nname = \"criterion\"\n\n[[bin]]\nname = \"tool-bin\"\npath = \"src/main.rs\"\n",
        )
        .unwrap();
        for (subdir, file) in [
            ("tests", "ui_tests.rs"),
            ("examples", "demo_example.rs"),
            ("benches", "fast_bench.rs"),
            ("src/bin", "helper_bin.rs"),
        ] {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
            std::fs::write(dir.join(subdir).join(file), "fn main() {}\n").unwrap();
        }

        let prefixes =
            rvs_detect_local_crate_prefixes_BIS(&dir, CargoTargetScope::Production).unwrap();
        let output = prefixes
            .iter()
            .map(CrateName::rvs_as_str)
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260706_detect_local_crate_prefixes_for_cargo_check_excludes_unchecked_targets",
            &output,
        );

        assert!(prefixes.contains(&CrateName::from("prod_prefix_demo")));
        assert!(prefixes.contains(&CrateName::from("tool_bin")));
        assert!(prefixes.contains(&CrateName::from("helper_bin")));
        assert!(!prefixes.contains(&CrateName::from("tokio")));
        assert!(!prefixes.contains(&CrateName::from("serde")));
        assert!(!prefixes.contains(&CrateName::from("criterion")));
        assert!(!prefixes.contains(&CrateName::from("ui_tests")));
        assert!(!prefixes.contains(&CrateName::from("demo_example")));
        assert!(!prefixes.contains(&CrateName::from("fast_bench")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_ensure_cargo_project_requires_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("cargo-check");

        let result = rvs_ensure_cargo_project_BIS(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_ensure_cargo_project_requires_cargo_toml",
            &output,
        );
        assert!(result.is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_prepare_cargo_check_matches_target_scope() {
        let dir = rvs_make_workspace_temp_dir_BIS("target-scope-command");
        let mut output = String::new();
        for (name, target_scope) in [
            ("production", CargoTargetScope::Production),
            ("all_targets", CargoTargetScope::WithTestExampleBench),
        ] {
            let mode = CargoCheckMode::Lint(CargoLintInput::Offline(OfflineLintInput {
                emissions: OfflineEmissionInput {
                    path: PathBuf::from("emissions.json"),
                    acknowledgement_dir: PathBuf::from("acks"),
                },
            }));
            let mut generation =
                rvs_reserve_cargo_check_test_generation_BIST(&dir, &mode, target_scope);
            let config = CargoCheckConfig {
                project_path: &dir,
                generation: &generation,
                mode,
                target_scope,
                extra_args: vec![],
                target_subdir: None,
            };
            let cmd = rvs_prepare_cargo_check_command_BIST(&config).unwrap();
            let args = cmd
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            output.push_str(&format!("{name}={}\n", args.join(" ")));
            rvs_cleanup_run_generation_BIMS(&mut generation)
                .expect("never: target-scope command generation cleanup should succeed");
        }
        rvs_snapshot_BIS(
            "test_20260713_prepare_cargo_check_matches_target_scope",
            &output,
        );

        assert_eq!(
            output,
            "production=check --profile test\nall_targets=check --profile test --all-targets\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_prepare_cargo_check_sanitizes_rivus_env() {
        let dir = rvs_make_workspace_temp_dir_BIS("sanitize-env-no-caps");
        let mode = CargoCheckMode::Lint(CargoLintInput::Offline(OfflineLintInput {
            emissions: OfflineEmissionInput {
                path: dir.join("emissions.json"),
                acknowledgement_dir: dir.join("acks"),
            },
        }));
        let mut generation = rvs_reserve_cargo_check_test_generation_BIST(
            &dir,
            &mode,
            CargoTargetScope::WithTestExampleBench,
        );
        let config = CargoCheckConfig {
            project_path: &dir,
            generation: &generation,
            mode,
            target_scope: CargoTargetScope::WithTestExampleBench,
            extra_args: vec![],
            target_subdir: None,
        };

        let cmd = rvs_prepare_cargo_check_command_BIST(&config).unwrap();
        let capsmap_state = match rvs_command_env_value(&cmd, "RIVUS_CAPSMAP") {
            Some(None) => "removed",
            Some(Some(path)) if Path::new(&path).is_absolute() => "absolute",
            Some(Some(_)) => "relative",
            None => "inherited",
        };
        let output = format!(
            "callgraph={:?}\ncapsmap={capsmap_state}\noffline_emissions={:?}\noffline_acks={:?}\nrustc={:?}\nrivus_enabled={:?}\nui_testing={:?}\nuntested_paths={:?}\n",
            rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH"),
            rvs_command_env_value(&cmd, "RIVUS_OFFLINE_EMISSIONS"),
            rvs_command_env_value(&cmd, "RIVUS_OFFLINE_EMISSIONS_ACK_DIR"),
            rvs_command_env_value(&cmd, "RUSTC"),
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
            rvs_command_env_value(&cmd, "RIVUS_UI_TESTING"),
            rvs_command_env_value(&cmd, "RIVUS_UNTESTED_PATHS"),
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260704_prepare_cargo_check_sanitizes_rivus_env",
            &output,
        );

        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_CALLGRAPH_DIR"),
            Some(None)
        );
        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_CAPSMAP"), Some(None));
        assert_eq!(rvs_command_env_value(&cmd, "RUSTC"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_CRATE_PROVENANCE"),
            Some(None)
        );
        assert_eq!(
            rvs_command_env_value(&cmd, "CARGO_PRIMARY_PACKAGE"),
            Some(None)
        );
        assert_eq!(rvs_command_env_value(&cmd, "RIVUS_UI_TESTING"), Some(None));
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_UNTESTED_PATHS"),
            Some(None)
        );
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_ENABLED"),
            Some(Some("1".to_string()))
        );
        assert_eq!(
            rvs_command_env_value(&cmd, "RIVUS_WRAPPER"),
            Some(Some("1".to_string()))
        );
        assert_eq!(rvs_command_env_value(&cmd, "RUSTC_WRAPPER"), Some(None));
        assert!(matches!(
            rvs_command_env_value(&cmd, "RUSTC_WORKSPACE_WRAPPER"),
            Some(Some(_))
        ));

        rvs_cleanup_run_generation_BIMS(&mut generation)
            .expect("never: sanitized command generation cleanup should succeed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260726_cargo_check_modes_own_driver_protocol() {
        let dir = rvs_make_workspace_temp_dir_BIS("typed-cargo-check-modes");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"typed-cargo-check-modes\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_value() {}\n").unwrap();
        let modes = [
            (
                "offline_lint",
                CargoCheckMode::Lint(CargoLintInput::Offline(OfflineLintInput {
                    emissions: OfflineEmissionInput {
                        path: PathBuf::from("emissions.json"),
                        acknowledgement_dir: PathBuf::from("acks"),
                    },
                })),
            ),
            (
                "workspace_callgraph",
                CargoCheckMode::Callgraph {
                    collection: CallgraphCollectionMode::Workspace,
                    artifact_dir: PathBuf::from("workspace-artifacts"),
                },
            ),
            (
                "all_crates_callgraph",
                CargoCheckMode::Callgraph {
                    collection: CallgraphCollectionMode::AllCrates,
                    artifact_dir: PathBuf::from("all-crates-artifacts"),
                },
            ),
            (
                "standard_library_callgraph",
                CargoCheckMode::Callgraph {
                    collection: CallgraphCollectionMode::StandardLibrary,
                    artifact_dir: PathBuf::from("std-artifacts"),
                },
            ),
        ];
        let mut output = String::new();
        for (name, mode) in modes {
            let mut generation = rvs_reserve_cargo_check_test_generation_BIST(
                &dir,
                &mode,
                CargoTargetScope::Production,
            );
            let config = CargoCheckConfig {
                project_path: &dir,
                generation: &generation,
                mode,
                target_scope: CargoTargetScope::Production,
                extra_args: vec![],
                target_subdir: None,
            };
            let cmd = rvs_prepare_cargo_check_command_BIST(&config).unwrap();
            let args = cmd
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let env_is_set =
                |key| rvs_command_env_value(&cmd, key).is_some_and(|value| value.is_some());
            let provenance = rvs_command_env_value(&cmd, "RIVUS_CRATE_PROVENANCE")
                .flatten()
                .unwrap_or_else(|| "none".to_string());
            output.push_str(&format!(
                "{name}: workspace_wrapper={} all_crates_wrapper={} callgraph={} provenance={provenance} offline={} emissions={} acks={} nightly={} build_std={} target={}\n",
                env_is_set("RUSTC_WORKSPACE_WRAPPER"),
                env_is_set("RUSTC_WRAPPER"),
                env_is_set("RIVUS_CALLGRAPH"),
                env_is_set("RIVUS_OFFLINE_CAPS"),
                env_is_set("RIVUS_OFFLINE_EMISSIONS"),
                env_is_set("RIVUS_OFFLINE_EMISSIONS_ACK_DIR"),
                env_is_set("RUSTUP_TOOLCHAIN"),
                args.iter().any(|arg| arg == "-Zbuild-std=std,core,alloc"),
                args.iter().any(|arg| arg == "--target"),
            ));
            rvs_cleanup_run_generation_BIMS(&mut generation)
                .expect("never: typed-mode command generation cleanup should succeed");
        }
        rvs_snapshot_BIS(
            "test_20260726_cargo_check_modes_own_driver_protocol",
            &output,
        );

        assert_eq!(
            output,
            "offline_lint: workspace_wrapper=true all_crates_wrapper=false callgraph=false provenance=none offline=true emissions=true acks=true nightly=false build_std=false target=false\n\
workspace_callgraph: workspace_wrapper=true all_crates_wrapper=false callgraph=true provenance=cargo-primary offline=false emissions=false acks=false nightly=false build_std=false target=false\n\
all_crates_callgraph: workspace_wrapper=false all_crates_wrapper=true callgraph=true provenance=cargo-primary offline=false emissions=false acks=false nightly=false build_std=false target=false\n\
standard_library_callgraph: workspace_wrapper=false all_crates_wrapper=true callgraph=true provenance=cargo-primary offline=false emissions=false acks=false nightly=true build_std=true target=true\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_rejects_caps_file() {
        let dir = rvs_make_workspace_temp_dir_BIS("caps-file");
        std::fs::write(dir.join("caps"), "bad=Z\n").unwrap();
        // Broken caps must fail the check before any cargo process spawns.
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!("is_err={}\ncode={:?}\n", result.is_err(), result.err());
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_rejects_caps_file",
            &output,
        );

        assert_eq!(result, Err(1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_prepare_offline_cargo_check_defers_caps_to_parent() {
        let dir = rvs_make_workspace_temp_dir_BIS("offline-caps-parent-snapshot");
        std::fs::write(dir.join("caps"), "bad=Z\n").unwrap();
        let mode = CargoCheckMode::Lint(CargoLintInput::Offline(OfflineLintInput {
            emissions: OfflineEmissionInput {
                path: PathBuf::from("emissions.json"),
                acknowledgement_dir: PathBuf::from("acks"),
            },
        }));
        let mut generation = rvs_reserve_cargo_check_test_generation_BIST(
            &dir,
            &mode,
            CargoTargetScope::WithTestExampleBench,
        );
        let config = CargoCheckConfig {
            project_path: &dir,
            generation: &generation,
            mode,
            target_scope: CargoTargetScope::WithTestExampleBench,
            extra_args: vec![],
            target_subdir: None,
        };

        let result = rvs_prepare_cargo_check_command_BIST(&config);
        let capsmap_env = result
            .as_ref()
            .ok()
            .and_then(|command| rvs_command_env_value(command, "RIVUS_CAPSMAP"))
            .flatten();
        let output = format!(
            "result_is_ok={}\ncapsmap_env={capsmap_env:?}\n",
            result.is_ok()
        );
        rvs_snapshot_BIS(
            "test_20260713_prepare_offline_cargo_check_defers_caps_to_parent",
            &output,
        );

        assert!(result.is_ok());
        assert!(capsmap_env.is_none());
        rvs_cleanup_run_generation_BIMS(&mut generation)
            .expect("never: offline command generation cleanup should succeed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_prepare_cargo_check_rejects_broken_project_caps_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("broken-project-caps-symlink");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"broken-caps-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(dir.join("missing-caps"), dir.join("caps")).unwrap();
        // A broken caps symlink must fail the check before any cargo process
        // spawns.
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!("is_err={}\ncode={:?}\n", result.is_err(), result.err());
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_rejects_broken_project_caps_symlink",
            &output,
        );

        assert_eq!(result, Err(1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_load_project_caps_rejects_broken_caps_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("load-broken-project-caps-symlink");
        std::os::unix::fs::symlink(dir.join("missing-caps"), dir.join("caps")).unwrap();

        let result = rvs_load_project_caps_BIS(&dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_load_project_caps_rejects_broken_caps_symlink",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("is not a directory"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_check_rejects_forwarded_manifest_path() {
        let equals_args = vec!["--manifest-path=other/Cargo.toml".to_string()];
        let split_args = vec![
            "--manifest-path".to_string(),
            "other/Cargo.toml".to_string(),
        ];
        let normal_args = vec!["--release".to_string()];

        let equals = rvs_reject_forwarded_check_args(&equals_args);
        let split = rvs_reject_forwarded_check_args(&split_args);
        let normal = rvs_reject_forwarded_check_args(&normal_args);
        let output = format!("equals={equals:?}\nsplit={split:?}\nnormal={normal:?}\n");
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_forwarded_manifest_path",
            &output,
        );

        assert!(equals.is_err());
        assert!(split.is_err());
        assert!(normal.is_ok());
    }

    #[test]
    fn test_20260714_check_rejects_forwarded_target_dir() {
        let split_args = vec!["--target-dir".to_string(), "custom-target".to_string()];
        let equals_args = vec!["--target-dir=custom-target".to_string()];
        let normal_args = vec!["--release".to_string()];
        let output = format!(
            "split={}\nequals={}\nnormal={}\n",
            rvs_reject_forwarded_check_args(&split_args).is_err(),
            rvs_reject_forwarded_check_args(&equals_args).is_err(),
            rvs_reject_forwarded_check_args(&normal_args).is_ok(),
        );
        rvs_snapshot_BIS("test_20260714_check_rejects_forwarded_target_dir", &output);

        assert!(rvs_reject_forwarded_check_args(&split_args).is_err());
        assert!(rvs_reject_forwarded_check_args(&equals_args).is_err());
        assert!(rvs_reject_forwarded_check_args(&normal_args).is_ok());
    }

    #[test]
    fn test_20260714_check_rejects_workspace_package_selectors() {
        let cases = [
            vec!["--workspace".to_string()],
            vec!["--all".to_string()],
            vec!["--package".to_string(), "member".to_string()],
            vec!["--package=member".to_string()],
            vec!["-p".to_string(), "member".to_string()],
            vec!["-pmember".to_string()],
            vec!["--exclude".to_string(), "member".to_string()],
            vec!["--exclude=member".to_string()],
        ];
        let output = cases
            .iter()
            .map(|args| {
                format!(
                    "{}={:?}",
                    args.join(" "),
                    rvs_reject_forwarded_check_args(args)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_workspace_package_selectors",
            &(output + "\n"),
        );

        assert!(
            cases
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
    }

    #[test]
    fn test_20260714_check_rejects_forwarded_target_selectors() {
        let cases = [
            vec!["--lib".to_string()],
            vec!["--bins".to_string()],
            vec!["--bin".to_string(), "tool".to_string()],
            vec!["--bin=tool".to_string()],
            vec!["--examples".to_string()],
            vec!["--example".to_string(), "demo".to_string()],
            vec!["--example=demo".to_string()],
            vec!["--tests".to_string()],
            vec!["--test".to_string(), "ui".to_string()],
            vec!["--test=ui".to_string()],
            vec!["--benches".to_string()],
            vec!["--bench".to_string(), "perf".to_string()],
            vec!["--bench=perf".to_string()],
            vec!["--all-targets".to_string()],
        ];
        let output = cases
            .iter()
            .map(|args| {
                format!(
                    "{}={}",
                    args.join(" "),
                    rvs_reject_forwarded_check_args(args).is_err()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_forwarded_target_selectors",
            &(output + "\n"),
        );

        assert!(
            cases
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
    }

    #[test]
    fn test_20260714_check_rejects_non_building_and_json_modes() {
        let rejected = [
            vec!["--help".to_string()],
            vec!["-h".to_string()],
            vec!["--version".to_string()],
            vec!["-V".to_string()],
            vec!["--unit-graph".to_string()],
            vec!["--build-plan".to_string()],
            vec!["--print".to_string(), "cfg".to_string()],
            vec!["--print=cfg".to_string()],
            vec!["--message-format".to_string(), "json".to_string()],
            vec!["--message-format=json-diagnostic-short".to_string()],
            vec!["--message-format=short,json".to_string()],
        ];
        let accepted = [
            vec!["--message-format".to_string(), "short".to_string()],
            vec!["--message-format=human".to_string()],
            vec!["--release".to_string()],
        ];
        let output = format!(
            "rejected={}\naccepted={}\n",
            rejected
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err()),
            accepted
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_ok()),
        );
        rvs_snapshot_BIS(
            "test_20260714_check_rejects_non_building_and_json_modes",
            &output,
        );

        assert!(
            rejected
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_err())
        );
        assert!(
            accepted
                .iter()
                .all(|args| rvs_reject_forwarded_check_args(args).is_ok())
        );
    }

    #[test]
    fn test_20260714_grouped_banned_import_reports_once() {
        let dir = rvs_make_workspace_temp_dir_BIS("grouped-banned-import");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("anyhow/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"banned-import-group\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nanyhow = { path = \"anyhow\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![feature(register_tool)]\n#![register_tool(rivus)]\n\nextern crate anyhow as anyhow_alias;\nuse anyhow::{Context, Error, Result};\nuse anyhow::Context as AllowedContext; use anyhow::Error as DeniedError;\n\nmacro_rules! import_anyhow { ($alias:ident) => { use anyhow::Context as $alias;     }\n}\nmod allowed_macro {\n    import_anyhow!(AllowedMacroContext);\n    const _: usize = core::mem::size_of::<AllowedMacroContext>();\n}\n\nimport_anyhow!(DeniedMacroContext);\n\nconst _: usize = core::mem::size_of::<(anyhow_alias::Error, Context, Error, Result, AllowedContext, DeniedError, DeniedMacroContext)>();\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("anyhow/Cargo.toml"),
            "[package]\nname = \"anyhow\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("anyhow/src/lib.rs"),
            "pub struct Context;\npub struct Error;\npub struct Result;\n",
        )
        .unwrap();

        let output = Command::new(rvs_current_wrapper_exe_BIS().unwrap())
            .arg("check")
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let warning_count = stderr
            .lines()
            .filter(|line| line.contains("warning: banned import: anyhow"))
            .count();
        let grouped_caret_width = stderr
            .lines()
            .skip_while(|line| !line.contains("use anyhow::{Context, Error, Result};"))
            .find(|line| line.contains('^'))
            .map(|line| line.chars().filter(|character| *character == '^').count())
            .unwrap_or(0);
        let allowed_macro_reported = stderr.contains("import_anyhow!(AllowedMacroContext)");
        let denied_macro_reported = stderr.contains("import_anyhow!(DeniedMacroContext)");
        let summary = format!(
            "success={}\nwarning_count={warning_count}\ngrouped_span_covers_statement={}\nallowed_macro_reported={allowed_macro_reported}\ndenied_macro_reported={denied_macro_reported}\n",
            output.status.success(),
            grouped_caret_width > "Context".len(),
        );
        rvs_snapshot_BIS("test_20260714_grouped_banned_import_reports_once", &summary);

        assert!(output.status.success(), "{stderr}");
        assert_eq!(warning_count, 6, "{stderr}");
        assert!(grouped_caret_width > "Context".len(), "{stderr}");
        assert!(allowed_macro_reported, "{stderr}");
        assert!(denied_macro_reported, "{stderr}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_check_rejects_dangerous_forwarded_config() {
        let build_rustc = vec![
            "--config".to_string(),
            "build.rustc=\"clippy-driver\"".to_string(),
        ];
        let wrapper = vec!["--config=build.rustc-wrapper=\"/tmp/w\"".to_string()];
        let rivus_env = vec![
            "--config".to_string(),
            "env.RIVUS_ENABLED={ value=\"0\", force=true }".to_string(),
        ];
        let ui_env = vec!["--config=env.RIVUS_UI_TESTING.value=\"1\"".to_string()];
        let coverage_env = vec!["--config=env.RIVUS_UNTESTED_PATHS.value=\"bad\"".to_string()];
        let emissions_env = vec!["--config=env.RIVUS_OFFLINE_EMISSIONS.value=\"bad\"".to_string()];
        let rustc_env = vec!["--config=env.RUSTC_WRAPPER.value=\"bad\"".to_string()];
        let path_config = vec!["--config".to_string(), "ci-cargo-config.toml".to_string()];
        let harmless = vec!["--config=net.offline=true".to_string()];

        let output = format!(
            "build_rustc={}\nwrapper={}\nrivus_env={}\nui_env={}\ncoverage_env={}\nemissions_env={}\nrustc_env={}\npath_config={}\nharmless={}\n",
            rvs_reject_forwarded_check_args(&build_rustc).is_err(),
            rvs_reject_forwarded_check_args(&wrapper).is_err(),
            rvs_reject_forwarded_check_args(&rivus_env).is_err(),
            rvs_reject_forwarded_check_args(&ui_env).is_err(),
            rvs_reject_forwarded_check_args(&coverage_env).is_err(),
            rvs_reject_forwarded_check_args(&emissions_env).is_err(),
            rvs_reject_forwarded_check_args(&rustc_env).is_err(),
            rvs_reject_forwarded_check_args(&path_config).is_err(),
            rvs_reject_forwarded_check_args(&harmless).is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_dangerous_forwarded_config",
            &output,
        );

        assert!(rvs_reject_forwarded_check_args(&build_rustc).is_err());
        assert!(rvs_reject_forwarded_check_args(&wrapper).is_err());
        assert!(rvs_reject_forwarded_check_args(&rivus_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&ui_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&coverage_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&emissions_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&rustc_env).is_err());
        assert!(rvs_reject_forwarded_check_args(&path_config).is_err());
        assert!(rvs_reject_forwarded_check_args(&harmless).is_ok());
        assert!(rvs_reject_dangerous_forwarded_config("build.rustc=\"clippy-driver\"").is_err());
        assert!(rvs_reject_dangerous_forwarded_config("net.offline=true").is_ok());
    }

    #[test]
    fn test_20260706_check_rejects_dangerous_forwarded_config_toml_keys() {
        let spaced = "build . rustc = \"bad\"";
        let quoted = "\"build\".\"rustc-wrapper\"=\"bad\"";
        let mixed_quote = "env.'RUSTC_WRAPPER'.value=\"bad\"";
        let escaped = "env.\"RUSTC\\u005fWRAPPER\".value=\"bad\"";
        let harmless_quoted = "\"net\".\"offline\"=true";
        let output = format!(
            "spaced={}\nquoted={}\nmixed_quote={}\nescaped={}\nharmless_quoted={}\n",
            rvs_reject_dangerous_forwarded_config(spaced).is_err(),
            rvs_reject_dangerous_forwarded_config(quoted).is_err(),
            rvs_reject_dangerous_forwarded_config(mixed_quote).is_err(),
            rvs_reject_dangerous_forwarded_config(escaped).is_err(),
            rvs_reject_dangerous_forwarded_config(harmless_quoted).is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260706_check_rejects_dangerous_forwarded_config_toml_keys",
            &output,
        );

        assert!(rvs_reject_dangerous_forwarded_config(spaced).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(quoted).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(mixed_quote).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(escaped).is_err());
        assert!(rvs_reject_dangerous_forwarded_config(harmless_quoted).is_ok());
        let doc = escaped.parse::<toml_edit::DocumentMut>().unwrap();
        let mut paths = Vec::new();
        for (key, item) in doc.iter() {
            rvs_collect_config_key_paths_M(key, item, &mut paths);
        }
        assert!(paths.iter().any(|path| path == "env.RUSTC_WRAPPER"));
    }

    #[test]
    fn test_20260706_clean_dir_removes_file_path() {
        let dir = rvs_make_workspace_temp_dir_BIS("clean-dir-file-path");
        let path = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "stale").unwrap();

        let result = rvs_clean_dir_BIS(&path);
        let exists = path.exists();
        let output = format!("result={result:?}\nexists={exists}\n");
        rvs_snapshot_BIS("test_20260706_clean_dir_removes_file_path", &output);

        assert!(result.is_ok());
        assert!(!exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_clean_dir_removes_broken_symlink() {
        let dir = rvs_make_workspace_temp_dir_BIS("clean-dir-broken-symlink");
        let path = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(dir.join("missing"), &path).unwrap();

        let result = rvs_clean_dir_BIS(&path);
        let symlink_exists = std::fs::symlink_metadata(&path).is_ok();
        let output = format!("result={result:?}\nsymlink_exists={symlink_exists}\n");
        rvs_snapshot_BIS("test_20260706_clean_dir_removes_broken_symlink", &output);

        assert!(result.is_ok());
        assert!(!symlink_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_write_capsmap_result_rejects_output_directory_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-dir-preflight");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("out-dir");
        std::fs::create_dir_all(&output_path).unwrap();
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_output_directory_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_output_without_file_name() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-no-file-name");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output = Some(PathBuf::new());

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n",)
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_output_without_file_name",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_parent_dir_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-parent-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join("..");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_parent_dir_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_internal_parent_dir_without_side_effects() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-internal-parent-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join("..").join("deps");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let missing_exists = dir.join("missing").exists();
        let deps_exists = dir.join("deps").exists();
        let output = format!(
            "result={result:?}\ndefault_exists={default_exists}\nmissing_exists={missing_exists}\ndeps_exists={deps_exists}\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_internal_parent_dir_without_side_effects",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(!missing_exists);
        assert!(!deps_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_current_dir_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-current-dir");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("missing").join(".");
        let output = Some(output_path);

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_current_dir_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_capsmap_result_rejects_trailing_slash_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-trailing-slash");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output = Some(PathBuf::from(format!("{}/out/", dir.to_string_lossy())));

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let output = format!("result={result:?}\ndefault_exists={default_exists}\n")
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_trailing_slash_output_before_default_write",
            &output,
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_file_preserves_existing_temp_collision() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-temp-collision");
        let path = dir.join("deps");
        let temp_path = dir.join(format!(".deps.{}.0.tmp", std::process::id()));
        std::fs::write(&temp_path, "old-temp\n").unwrap();

        let result = rvs_write_capsmap_file_BIST(&path, "new=BI\n", "deps capsmap");
        let final_text = std::fs::read_to_string(&path).unwrap_or_default();
        let temp_text = std::fs::read_to_string(&temp_path).unwrap_or_default();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_file_preserves_existing_temp_collision",
            &format!("result={result:?}\nfinal={final_text:?}\ntemp={temp_text:?}\n"),
        );

        assert!(result.is_ok());
        assert_eq!(final_text, "new=BI\n");
        assert_eq!(temp_text, "old-temp\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_result_rejects_symlink_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-symlink");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_target = dir.join("output-target-dir");
        let output_path = dir.join("output-link");
        std::fs::create_dir_all(&output_target).unwrap();
        std::os::unix::fs::symlink(&output_target, &output_path).unwrap();
        let output = Some(output_path.clone());

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let symlink_exists = std::fs::symlink_metadata(&output_path).is_ok();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_symlink_output_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\nsymlink_exists={symlink_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(symlink_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_write_capsmap_result_rejects_socket_output_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-output-socket");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let output_path = dir.join("output-socket");
        let listener = std::os::unix::net::UnixListener::bind(&output_path).unwrap();
        let output = Some(output_path.clone());

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        let socket_exists = std::fs::symlink_metadata(&output_path).is_ok();
        rvs_snapshot_BIS(
            "test_20260707_write_capsmap_result_rejects_socket_output_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\nsocket_exists={socket_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        assert!(socket_exists);
        drop(listener);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_capsmap_result_rejects_broken_parent_symlink_before_default_write() {
        let dir = rvs_make_workspace_temp_dir_BIS("capsmap-broken-parent-symlink");
        let default_path = dir.join("target/rivus-std-capsmap.txt");
        let parent_link = dir.join("missing-parent-link");
        std::os::unix::fs::symlink(dir.join("missing-parent"), &parent_link).unwrap();
        let output = Some(parent_link.join("capsmap"));

        let result = rvs_write_capsmap_result_BIST(
            "new=BI\n",
            output.as_deref().unwrap_or(&default_path),
            "std capsmap",
        );
        let default_exists = default_path.exists();
        rvs_snapshot_BIS(
            "test_20260706_write_capsmap_result_rejects_broken_parent_symlink_before_default_write",
            &format!(
                "result_is_err={}\ndefault_exists={default_exists}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(!default_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_prepare_cargo_check_validates_project_caps_without_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("invalid-project-caps-no-std-cache");
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/ext"),
            "# rivus-caps-v2\n{\"path\":\"bad\",\"caps\":\"Z\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
        )
        .unwrap();
        // Invalid caps records must fail the check before any cargo process
        // spawns.
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!("is_err={}\ncode={:?}\n", result.is_err(), result.err());
        rvs_snapshot_BIS(
            "test_20260706_prepare_cargo_check_validates_project_caps_without_std_cache",
            &output,
        );

        assert_eq!(result, Err(1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_prepare_cargo_check_uses_absolute_paths() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let relative_project = PathBuf::from("target").join(format!(
            "rivus-relative-project-{}-{unique}",
            std::process::id()
        ));
        let absolute_project = std::env::current_dir().unwrap().join(&relative_project);
        std::fs::create_dir_all(absolute_project.join("caps")).unwrap();

        let mode = CargoCheckMode::Lint(CargoLintInput::Offline(OfflineLintInput {
            emissions: OfflineEmissionInput {
                path: absolute_project.join("emissions.json"),
                acknowledgement_dir: absolute_project.join("acks"),
            },
        }));
        let mut generation = rvs_reserve_cargo_check_test_generation_BIST(
            &relative_project,
            &mode,
            CargoTargetScope::WithTestExampleBench,
        );
        let config = CargoCheckConfig {
            project_path: &relative_project,
            generation: &generation,
            mode,
            target_scope: CargoTargetScope::WithTestExampleBench,
            extra_args: vec![],
            target_subdir: Some("rivus-custom-build"),
        };

        let cmd = rvs_prepare_cargo_check_command_BIST(&config).unwrap();
        let current_dir = cmd.get_current_dir().expect("command should set cwd");
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let target_dir = args
            .windows(2)
            .find_map(|window| (window[0] == "--target-dir").then(|| PathBuf::from(&window[1])))
            .expect("command should set target dir");
        let emissions_path = rvs_command_env_value(&cmd, "RIVUS_OFFLINE_EMISSIONS")
            .and_then(|value| value)
            .map(PathBuf::from)
            .expect("offline emissions should be configured");
        let output = format!(
            "cwd_abs={}\ntarget_abs={}\nemissions_abs={}\n",
            current_dir.is_absolute(),
            target_dir.is_absolute(),
            emissions_path.is_absolute(),
        );
        rvs_snapshot_BIS(
            "test_20260704_prepare_cargo_check_uses_absolute_paths",
            &output,
        );

        assert!(current_dir.is_absolute());
        assert!(target_dir.is_absolute());
        assert!(emissions_path.is_absolute());
        assert!(target_dir.ends_with("target/rivus-custom-build"));
        assert!(emissions_path.ends_with("emissions.json"));

        rvs_cleanup_run_generation_BIMS(&mut generation)
            .expect("never: absolute-path command generation cleanup should succeed");
        std::fs::remove_dir_all(absolute_project).unwrap();
    }

    #[test]
    fn test_20260715_callgraph_generations_are_sibling_safe() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-generation-isolation");
        let (mut first, mut second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| rvs_reserve_run_generation_BIST(&dir, "callgraph").unwrap());
            let second =
                scope.spawn(|| rvs_reserve_run_generation_BIST(&dir, "callgraph").unwrap());
            (first.join().unwrap(), second.join().unwrap())
        });
        std::fs::write(second.rvs_root().join("sentinel"), "active\n").unwrap();

        let distinct = first.rvs_root() != second.rvs_root();
        let artifact_dirs_are_absolute =
            first.rvs_artifact_dir().is_absolute() && second.rvs_artifact_dir().is_absolute();
        let target_dirs_are_distinct = first.rvs_target_subdir() != second.rvs_target_subdir();
        rvs_cleanup_run_generation_BIMS(&mut first).unwrap();
        let sibling_preserved = second.rvs_root().join("sentinel").is_file();
        let first_removed = !first.rvs_root().exists();
        rvs_cleanup_run_generation_BIMS(&mut second).unwrap();
        let output = format!(
            "distinct={distinct}\nartifact_dirs_are_absolute={artifact_dirs_are_absolute}\ntarget_dirs_are_distinct={target_dirs_are_distinct}\nfirst_removed={first_removed}\nsibling_preserved={sibling_preserved}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_callgraph_generations_are_sibling_safe",
            &output,
        );

        assert!(distinct);
        assert!(artifact_dirs_are_absolute);
        assert!(target_dirs_are_distinct);
        assert!(first_removed);
        assert!(sibling_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260717_infer_std_collection_uses_dependency_free_probe() {
        use std::os::unix::fs::PermissionsExt as _;

        const CHILD_ENV: &str = "RVS_TEST_STD_PROBE_CHILD";
        const PROJECT_ENV: &str = "RVS_TEST_STD_PROBE_PROJECT";
        const CWD_ENV: &str = "RVS_TEST_STD_PROBE_CWD";
        const MANIFEST_ENV: &str = "RVS_TEST_STD_PROBE_MANIFEST";
        const REAL_CARGO_ENV: &str = "RVS_TEST_STD_PROBE_REAL_CARGO";

        if std::env::var_os(CHILD_ENV).is_some() {
            let project = PathBuf::from(
                std::env::var_os(PROJECT_ENV)
                    .expect("never: std probe child receives the project path"),
            );
            let result = rvs_collect_callgraph_BIST(
                &project,
                CallgraphCollectionMode::StandardLibrary,
                CargoTargetScope::Production,
                &BTreeSet::from([CrateName::from("std-probe-host")]),
            );
            assert!(
                matches!(result, Err(ref error) if error.contains("exit code 71")),
                "{result:?}"
            );
            return;
        }

        let dir = rvs_make_workspace_temp_dir_BIS("infer-std-dependency-free-probe");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"std-probe-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nexpensive-dependency = { path = \"expensive-dependency\" }\n",
        )
        .unwrap();
        let cwd_path = dir.join("cargo-cwd");
        let manifest_path = dir.join("cargo-manifest");
        let fake_cargo = dir.join("fake-cargo");
        std::fs::write(
            &fake_cargo,
            "#!/bin/sh\nif [ \"$1\" = metadata ]; then\n  exec \"$RVS_TEST_STD_PROBE_REAL_CARGO\" \"$@\"\nfi\npwd > \"$RVS_TEST_STD_PROBE_CWD\"\ncp Cargo.toml \"$RVS_TEST_STD_PROBE_MANIFEST\"\nexit 71\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_cargo, std::fs::Permissions::from_mode(0o700)).unwrap();

        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(
                "environment::workspace::tests::test_20260717_infer_std_collection_uses_dependency_free_probe",
            )
            .arg("--nocapture")
            .env(CHILD_ENV, "1")
            .env(PROJECT_ENV, &dir)
            .env(CWD_ENV, &cwd_path)
            .env(MANIFEST_ENV, &manifest_path)
            .env(REAL_CARGO_ENV, rvs_cargo_command_from_env_BS())
            .env("CARGO", &fake_cargo)
            .status()
            .unwrap();
        let cargo_cwd = PathBuf::from(std::fs::read_to_string(&cwd_path).unwrap().trim());
        let cargo_manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let cwd_is_probe = cargo_cwd != dir;
        let dependency_free = !cargo_manifest.contains("[dependencies]");
        let standalone = cargo_manifest.contains("[workspace]");
        let std_debug_assertions_disabled =
            cargo_manifest.contains("[profile.dev]\ndebug-assertions = false");
        let output = format!(
            "child_success={}\ncwd_is_probe={cwd_is_probe}\ndependency_free={dependency_free}\nstandalone={standalone}\nstd_debug_assertions_disabled={std_debug_assertions_disabled}\n",
            child.success(),
        );
        rvs_snapshot_BIS(
            "test_20260717_infer_std_collection_uses_dependency_free_probe",
            &output,
        );

        assert!(child.success());
        assert!(cwd_is_probe);
        assert!(dependency_free);
        assert!(standalone);
        assert!(std_debug_assertions_disabled);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_concurrent_callgraph_collections_do_not_mix_generations() {
        let dir = rvs_make_cargo_project_BIS(
            "concurrent-callgraph-generations",
            "concurrent-callgraph-generations",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub fn rvs_common() {}\n\n#[cfg(feature = \"first\")]\npub fn rvs_first() {}\n\n#[cfg(feature = \"second\")]\npub fn rvs_second() {}\n",
            )],
        );
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"concurrent-callgraph-generations\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nfirst = []\nsecond = []\n",
        )
        .unwrap();
        let local_crate_names =
            BTreeSet::from([CrateName::from("concurrent_callgraph_generations")]);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                rvs_collect_callgraph_with_args_BIST(
                    &dir,
                    CallgraphCollectionMode::Workspace,
                    CargoTargetScope::Production,
                    vec!["--features", "first"],
                    &local_crate_names,
                )
            });
            let second = scope.spawn(|| {
                rvs_collect_callgraph_with_args_BIST(
                    &dir,
                    CallgraphCollectionMode::Workspace,
                    CargoTargetScope::Production,
                    vec!["--features", "second"],
                    &local_crate_names,
                )
            });
            (
                first.join().unwrap().unwrap(),
                second.join().unwrap().unwrap(),
            )
        });
        let first_isolated = first
            .rvs_get("concurrent_callgraph_generations::rvs_first")
            .is_some()
            && first
                .rvs_get("concurrent_callgraph_generations::rvs_second")
                .is_none();
        let second_isolated = second
            .rvs_get("concurrent_callgraph_generations::rvs_second")
            .is_some()
            && second
                .rvs_get("concurrent_callgraph_generations::rvs_first")
                .is_none();
        let generations_remaining = std::fs::read_dir(dir.join("target/.rivus-runs"))
            .unwrap()
            .count();
        let output = format!(
            "first_isolated={first_isolated}\nsecond_isolated={second_isolated}\ngenerations_remaining={generations_remaining}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_concurrent_callgraph_collections_do_not_mix_generations",
            &output,
        );

        assert!(first_isolated);
        assert!(second_isolated);
        assert_eq!(generations_remaining, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_specialized_impls_keep_distinct_callgraph_identity() {
        let dir = rvs_make_cargo_project_BIS(
            "specialized-impl-identity",
            "specialized-impl-identity",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub struct Worker<T>(pub T);\n\n#[cfg(test)]\nimpl Worker<i8> {\n    pub fn rvs_test_only(&self) {}\n}\n\nimpl Worker<u8> {\n    pub fn rvs_run(&self) {\n        fn rvs_nested() {}\n        rvs_nested();\n    }\n}\n\nimpl Worker<u16> {\n    pub fn rvs_run(&self) {\n        fn rvs_nested() {}\n        rvs_nested();\n    }\n}\n\npub fn rvs_call_u8(worker: &Worker<u8>) { worker.rvs_run(); }\npub fn rvs_call_u16(worker: &Worker<u16>) { worker.rvs_run(); }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_20260715_covers_only_u8_specialization() {\n        rvs_call_u8(&Worker(1u8));\n    }\n}\n",
            )],
        );
        let local_crate_names = BTreeSet::from([CrateName::from("specialized_impl_identity")]);
        let callgraph = rvs_collect_workspace_callgraph_BIST(
            &dir,
            CargoTargetScope::WithTestExampleBench,
            &local_crate_names,
        )
        .unwrap();
        let method_paths = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_run")
            .cloned()
            .collect::<Vec<_>>();
        let nested_paths = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_nested")
            .cloned()
            .collect::<Vec<_>>();
        let call_u8 = callgraph
            .rvs_get("specialized_impl_identity::rvs_call_u8")
            .unwrap();
        let call_u16 = callgraph
            .rvs_get("specialized_impl_identity::rvs_call_u16")
            .unwrap();
        let analysis = crate::inference::PreparedLocalAnalysis::rvs_prepare(
            &callgraph,
            &capsmap::CapsMap::rvs_new(),
            &local_crate_names,
        );
        let uncovered = crate::offline_caps::rvs_uncovered_test_functions(
            &callgraph,
            &analysis,
            &local_crate_names,
        );
        let uncovered_methods = uncovered
            .keys()
            .filter(|identity| identity.def_path.rvs_fn_name_str() == "rvs_run")
            .map(|identity| &identity.def_path)
            .collect::<BTreeSet<_>>()
            .len();
        let display_paths = method_paths
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let nested_call_targets_distinct = method_paths
            .iter()
            .filter_map(|path| callgraph.rvs_get(path.rvs_as_str()))
            .map(|node| &node.calls)
            .collect::<BTreeSet<_>>()
            .len()
            == 2;
        let output = format!(
            "method_nodes={}\nraw_paths_distinct={}\ndisplay_paths_same={}\ncall_targets_distinct={}\nnested_nodes={}\nnested_call_targets_distinct={nested_call_targets_distinct}\nuncovered_methods={uncovered_methods}\n",
            method_paths.len(),
            method_paths.windows(2).all(|pair| pair[0] != pair[1]),
            display_paths.windows(2).all(|pair| pair[0] == pair[1]),
            call_u8.calls != call_u16.calls,
            nested_paths.len(),
        );
        rvs_snapshot_BIS(
            "test_20260715_specialized_impls_keep_distinct_callgraph_identity",
            &output,
        );

        assert_eq!(method_paths.len(), 2);
        assert!(method_paths.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(display_paths.windows(2).all(|pair| pair[0] == pair[1]));
        assert_ne!(call_u8.calls, call_u16.calls);
        assert_eq!(nested_paths.len(), 2);
        assert!(nested_call_targets_distinct);
        assert_eq!(uncovered_methods, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_dependency_impl_identity_matches_consumer_call_target() {
        let dir = rvs_make_workspace_temp_dir_BIS("dependency-impl-identity");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("fixture-dep/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"local-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-dep = { path = \"fixture-dep\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "#![allow(non_snake_case)]\n\nmod nested { pub struct Worker<T>(pub T); }\n\npub fn rvs_call_dependency(worker: &fixture_dep::nested::Worker<u8>) { worker.rvs_run(); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/Cargo.toml"),
            "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fixture-dep/src/lib.rs"),
            "#![allow(non_snake_case)]\n\npub mod nested {\n    pub struct Worker<T>(pub T);\n\n    impl Worker<u8> {\n        pub fn rvs_run(&self) {}\n    }\n}\n",
        )
        .unwrap();

        let local_crate_names = BTreeSet::from([CrateName::from("local-app")]);
        let callgraph = rvs_collect_callgraph_BIST(
            &dir,
            CallgraphCollectionMode::AllCrates,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let dependency_methods = callgraph
            .rvs_keys()
            .filter(|path| {
                path.rvs_as_str().starts_with("fixture_dep::")
                    && path.rvs_fn_name_str() == "rvs_run"
            })
            .cloned()
            .collect::<Vec<_>>();
        let dependency_method = dependency_methods
            .first()
            .expect("never: dependency method was collected");
        let caller = callgraph.rvs_get("local_app::rvs_call_dependency").unwrap();
        let output = format!(
            "dependency_nodes={}\ncall_matches_dependency_node={}\nreadable_path={}\n",
            dependency_methods.len(),
            caller
                .calls
                .keys()
                .any(|identity| identity.def_path == *dependency_method),
            dependency_method,
        );
        rvs_snapshot_BIS(
            "test_20260715_dependency_impl_identity_matches_consumer_call_target",
            &output,
        );

        assert_eq!(dependency_methods.len(), 1);
        assert!(
            caller
                .calls
                .keys()
                .any(|identity| identity.def_path == *dependency_method)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_trait_impl_nested_paths_preserve_nominal_identity() {
        let dir = rvs_make_cargo_project_BIS(
            "trait-impl-nested-identity",
            "trait-impl-nested-identity",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub mod a { pub struct Worker; }\npub mod b { pub struct Worker; }\n\npub trait Runner { fn rvs_run(&self); }\npub trait GenericRunner<T> { fn rvs_generic(&self); }\npub struct GenericWorker;\n\nmod implementations {\n    impl crate::Runner for crate::a::Worker {\n        fn rvs_run(&self) {\n            fn rvs_nested() {}\n            rvs_nested();\n        }\n    }\n\n    impl crate::Runner for crate::b::Worker {\n        fn rvs_run(&self) {\n            fn rvs_nested() {}\n            rvs_nested();\n        }\n    }\n\n    impl crate::GenericRunner<u8> for crate::GenericWorker {\n        fn rvs_generic(&self) {\n            fn rvs_generic_nested() {}\n            rvs_generic_nested();\n        }\n    }\n\n    impl crate::GenericRunner<u16> for crate::GenericWorker {\n        fn rvs_generic(&self) {\n            fn rvs_generic_nested() {}\n            rvs_generic_nested();\n        }\n    }\n}\n",
            )],
        );
        let local_crate_names = BTreeSet::from([CrateName::from("trait_impl_nested_identity")]);
        let callgraph = rvs_collect_workspace_callgraph_BIST(
            &dir,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let impl_methods = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_run")
            .filter(|path| path.rvs_trait_method_identity().is_some())
            .cloned()
            .collect::<Vec<_>>();
        let nested_paths = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_nested")
            .cloned()
            .collect::<Vec<_>>();
        let generic_methods = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_generic")
            .filter(|path| path.rvs_trait_method_identity().is_some())
            .cloned()
            .collect::<Vec<_>>();
        let generic_nested_paths = callgraph
            .rvs_keys()
            .filter(|path| path.rvs_fn_name_str() == "rvs_generic_nested")
            .cloned()
            .collect::<Vec<_>>();
        let readable_methods = impl_methods
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        let nested_call_targets_distinct = impl_methods
            .iter()
            .filter_map(|path| callgraph.rvs_get(path.rvs_as_str()))
            .map(|node| &node.calls)
            .collect::<BTreeSet<_>>()
            .len()
            == 2;
        let output = format!(
            "impl_methods={}\nreadable_methods_distinct={}\nnominal_a_present={}\nnominal_b_present={}\nnested_nodes={}\nnested_trait_identities={}\nnested_call_targets_distinct={nested_call_targets_distinct}\ngeneric_impl_methods={}\ngeneric_raw_paths_distinct={}\ngeneric_nested_nodes={}\n",
            impl_methods.len(),
            readable_methods.len() == 2,
            readable_methods
                .iter()
                .any(|path| path.contains("::a::Worker::rvs_run")),
            readable_methods
                .iter()
                .any(|path| path.contains("::b::Worker::rvs_run")),
            nested_paths.len(),
            nested_paths
                .iter()
                .filter(|path| path.rvs_trait_method_identity().is_some())
                .count(),
            generic_methods.len(),
            generic_methods.windows(2).all(|pair| pair[0] != pair[1]),
            generic_nested_paths.len(),
        );
        rvs_snapshot_BIS(
            "test_20260715_trait_impl_nested_paths_preserve_nominal_identity",
            &output,
        );

        assert_eq!(impl_methods.len(), 2);
        assert_eq!(readable_methods.len(), 2);
        assert_eq!(nested_paths.len(), 2);
        assert!(
            nested_paths
                .iter()
                .all(|path| path.rvs_trait_method_identity().is_none())
        );
        assert!(nested_call_targets_distinct);
        assert_eq!(generic_methods.len(), 2);
        assert!(generic_methods.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(generic_nested_paths.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_local_trait_impl_for_external_type_remains_in_scope() {
        let dir = rvs_make_cargo_project_BIS(
            "local-trait-external-type",
            "local-trait-external-type",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub trait TouchFile {\n    type World;\n    fn rvs_touch_P(world: &mut Self::World);\n}\n\nimpl TouchFile for std::fs::File {\n    type World = ();\n    fn rvs_touch_P(_world: &mut Self::World) {}\n}\n",
            )],
        );
        let local_crate_names = BTreeSet::from([CrateName::from("local_trait_external_type")]);
        let mut callgraph = rvs_collect_workspace_callgraph_BIST(
            &dir,
            CargoTargetScope::Production,
            &local_crate_names,
        )
        .unwrap();
        let _analysis = crate::inference::PreparedLocalAnalysis::rvs_prepare_M(
            &mut callgraph,
            &CapsMap::rvs_new(),
            &local_crate_names,
        );
        let (method_path, method_node) = callgraph
            .rvs_iter()
            .find(|(path, node)| path.rvs_fn_name_str() == "rvs_touch_P" && node.is_trait_impl)
            .expect("never: local trait implementation method was collected");
        let classification = crate::function_classification::FunctionClassification::rvs_new(
            &LocalScope::rvs_new(&local_crate_names),
            method_path,
            method_node,
        );
        let output = format!(
            "readable={method_path}\nport={}\noffline={}\nreport={}\n",
            method_node.facts.is_port_method,
            classification.rvs_is_offline_checked(),
            classification.rvs_is_report_candidate(),
        );
        rvs_snapshot_BIS(
            "test_20260715_local_trait_impl_for_external_type_remains_in_scope",
            &output,
        );

        assert!(method_node.facts.is_port_method);
        assert!(classification.rvs_is_offline_checked());
        assert!(classification.rvs_is_report_candidate());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_published_std_callgraph_precedes_legacy_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("published-std-callgraph-precedence");
        let legacy_dir = dir.join("target/rivus-callgraph-std");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let mut legacy = FnGraph::rvs_new();
        legacy.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_legacy"),
            rvs_targeted_test_node(1),
        );
        std::fs::write(
            legacy_dir.join("legacy.json"),
            crate::artifacts::rvs_serialize_callgraph_json(&legacy).unwrap(),
        )
        .unwrap();
        let mut published = FnGraph::rvs_new();
        published.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_published"),
            rvs_targeted_test_node(2),
        );
        crate::environment::callgraph_cache::rvs_publish_std_callgraph_cache_BIST(&dir, &published)
            .unwrap();

        let loaded = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let published_present = loaded.rvs_get("std::rvs_published").is_some();
        let legacy_present = loaded.rvs_get("std::rvs_legacy").is_some();
        let cache_is_file = dir.join("target/rivus-callgraph-std.json").is_file();
        let output = format!(
            "published_present={published_present}\nlegacy_present={legacy_present}\ncache_is_file={cache_is_file}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_published_std_callgraph_precedes_legacy_directory",
            &output,
        );

        assert!(published_present);
        assert!(!legacy_present);
        assert!(cache_is_file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_std_callgraph_publish_replaces_previous_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("replace-std-callgraph-cache");
        let mut previous = FnGraph::rvs_new();
        previous.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_previous"),
            rvs_targeted_test_node(1),
        );
        crate::environment::callgraph_cache::rvs_publish_std_callgraph_cache_BIST(&dir, &previous)
            .unwrap();
        let mut replacement = FnGraph::rvs_new();
        replacement.rvs_insert_M(
            crate::symbols::DefPath::from("std::rvs_replacement"),
            rvs_targeted_test_node(2),
        );

        crate::environment::callgraph_cache::rvs_publish_std_callgraph_cache_BIST(
            &dir,
            &replacement,
        )
        .unwrap();
        let loaded = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let previous_present = loaded.rvs_get("std::rvs_previous").is_some();
        let replacement_present = loaded.rvs_get("std::rvs_replacement").is_some();
        let output = format!(
            "previous_present={previous_present}\nreplacement_present={replacement_present}\n"
        );
        rvs_snapshot_BIS(
            "test_20260715_std_callgraph_publish_replaces_previous_cache",
            &output,
        );

        assert!(!previous_present);
        assert!(replacement_present);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260728_published_std_cache_preserves_support_inference_for_why() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-cache-support-inference");
        let support_path = crate::symbols::DefPath::from("support_crate::help");
        let published = rvs_support_inference_test_graph();
        crate::environment::callgraph_cache::rvs_publish_std_callgraph_cache_BIST(&dir, &published)
            .unwrap();
        // The ffi boundary is known through caps knowledge, not through its
        // name: suffixes are views over semantic caps, never sources.
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/ext"),
            rvs_caps_v2(&[("ffi_support::rvs_read_BI", "BI")]),
        )
        .unwrap();

        let (mut loaded, caps) = rvs_load_callgraph_and_caps_for_function_BIST(
            &dir,
            "std::fs::read_to_string",
            CargoTargetScope::WithTestExampleBench,
            &BTreeSet::new(),
        )
        .unwrap();
        let support_present = loaded.rvs_get(support_path.rvs_as_str()).is_some();
        let analysis = crate::inference::PreparedLocalAnalysis::rvs_prepare_M(
            &mut loaded,
            &caps,
            &BTreeSet::new(),
        );
        let resolver = analysis.rvs_resolver(&loaded, &caps);
        let support_caps = resolver
            .rvs_for_contract_check(&support_path)
            .map(|caps| caps.rvs_letters())
            .unwrap_or_else(|| "unknown".into());
        let std_present = loaded.rvs_get("std::fs::read_to_string").is_some();
        let output = format!(
            "std_present={std_present}\nsupport_present={support_present}\nsupport_caps={support_caps}\n"
        );
        rvs_snapshot_BIS(
            "test_20260728_published_std_cache_preserves_support_inference_for_why",
            &output,
        );

        assert!(std_present);
        assert!(support_present);
        assert_eq!(support_caps, "BI");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260728_std_why_cache_respects_current_caps_override() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-cache-current-caps-override");
        let support_path = crate::symbols::DefPath::from("support_crate::help");
        let published = rvs_support_inference_test_graph();
        crate::environment::callgraph_cache::rvs_publish_std_callgraph_cache_BIST(&dir, &published)
            .unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/ext"),
            rvs_caps_v2(&[(support_path.rvs_as_str(), "S")]),
        )
        .unwrap();

        let (mut loaded, caps) = rvs_load_callgraph_and_caps_for_function_BIST(
            &dir,
            "std::fs::read_to_string",
            CargoTargetScope::WithTestExampleBench,
            &BTreeSet::new(),
        )
        .unwrap();
        let analysis = crate::inference::PreparedLocalAnalysis::rvs_prepare_M(
            &mut loaded,
            &caps,
            &BTreeSet::new(),
        );
        let resolver = analysis.rvs_resolver(&loaded, &caps);
        let support_caps = resolver
            .rvs_for_contract_check(&support_path)
            .map(|caps| caps.rvs_letters())
            .unwrap_or_else(|| "unknown".into());
        let support_present = loaded.rvs_get(support_path.rvs_as_str()).is_some();
        let output = format!("support_present={support_present}\nsupport_caps={support_caps}\n");
        rvs_snapshot_BIS(
            "test_20260728_std_why_cache_respects_current_caps_override",
            &output,
        );

        assert!(support_present);
        assert_eq!(support_caps, "S");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_load_std_only_cached_callgraph() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-callgraph");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let callgraph = rvs_load_required_std_callgraph_cache_BIS(&dir).unwrap();
        let has_std = callgraph.rvs_get("std::fs::rvs_read_BI").is_some();
        rvs_snapshot_BIS(
            "test_20260704_load_std_only_cached_callgraph",
            &format!("has_std={has_std}\nlen={}\n", callgraph.rvs_len()),
        );

        assert!(has_std);
        assert_eq!(callgraph.rvs_len(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_std_only_mode_requires_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-missing-cache");

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS("test_20260704_std_only_mode_requires_std_cache", &output);

        assert!(result.is_err());
        assert!(output.contains("run cargo rivus infer-std first"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_std_only_rejects_callgraph_cache_file_path() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-callgraph-cache-file");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target/rivus-callgraph-std"), "stale").unwrap();

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_std_only_rejects_callgraph_cache_file_path",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("is not a directory"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_std_only_mode_ignores_project_cache_without_std_cache() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-only-project-cache-no-std");
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_load_required_std_callgraph_cache_BIS(&dir);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_std_only_mode_ignores_project_cache_without_std_cache",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("run cargo rivus infer-std first"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_is_std_like_def_path() {
        let cases = [
            ("std::fs::read", true),
            ("core::mem::drop", true),
            ("alloc::vec::Vec::new", true),
            ("compiler_builtins::mem::memcpy", true),
            ("demo::rvs_run", false),
            ("stdx::fs::read", false),
            ("corex::mem::drop", false),
            ("allocx::vec::Vec::new", false),
            ("compiler_builtinsx::mem::memcpy", false),
        ];
        let output = format!(
            "std={}\ncore={}\nalloc={}\ncompiler_builtins={}\nlocal={}\n",
            rvs_is_std_like_def_path("std::fs::read"),
            rvs_is_std_like_def_path("core::mem::drop"),
            rvs_is_std_like_def_path("alloc::vec::Vec::new"),
            rvs_is_std_like_def_path("compiler_builtins::mem::memcpy"),
            rvs_is_std_like_def_path("demo::rvs_run"),
        );
        rvs_snapshot_BIS("test_20260704_is_std_like_def_path", &output);

        for (path, expected) in cases {
            assert_eq!(rvs_is_std_like_def_path(path), expected, "{path}");
        }
    }

    #[test]
    fn test_20260710_std_like_query_matching_local_crate_uses_project_collection() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-local-crate");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"std\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let local_names =
            rvs_load_local_crate_prefixes_BIS(&dir, CargoTargetScope::WithTestExampleBench)
                .unwrap();
        let matches_local = LocalScope::rvs_new(&local_names).rvs_contains_str("std::rvs_run");
        let local_uses_std_cache = rvs_should_use_required_std_cache("std::rvs_run", &local_names);
        let real_std_uses_std_cache =
            rvs_should_use_required_std_cache("core::mem::drop", &local_names);
        let output = format!(
            "matches_local={matches_local}\nlocal_uses_std_cache={local_uses_std_cache}\nreal_std_uses_std_cache={real_std_uses_std_cache}\n",
        );
        rvs_snapshot_BIS(
            "test_20260710_std_like_query_matching_local_crate_uses_project_collection",
            &output,
        );

        assert!(matches_local);
        assert!(!local_uses_std_cache);
        assert!(real_std_uses_std_cache);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_std_like_query_mode_reports_invalid_cargo_toml() {
        let dir = rvs_make_workspace_temp_dir_BIS("std-like-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"std\"\n").unwrap();

        let result = crate::environment::cargo_targets::rvs_detect_local_crate_prefixes_for_function_query_BIS(
            &dir,
            CargoTargetScope::WithTestExampleBench,
        );
        rvs_snapshot_BIS(
            "test_20260706_std_like_query_mode_reports_invalid_cargo_toml",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_merge_std_like_callgraph_filters_local_nodes() {
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("demo::rvs_run".into(), crate::artifacts::FnNode::default());
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M(
            "demo::rvs_stale".into(),
            crate::artifacts::FnNode::default(),
        );
        source.rvs_insert_M(
            "std::fs::read_to_string".into(),
            crate::artifacts::FnNode::default(),
        );

        rvs_merge_std_like_callgraph_M(&mut target, &source).unwrap();
        let output = format!(
            "has_local={}\nhas_std={}\nlen={}\n",
            target.rvs_get("demo::rvs_stale").is_some(),
            target.rvs_get("std::fs::read_to_string").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260704_merge_std_like_callgraph_filters_local_nodes",
            &output,
        );

        assert!(target.rvs_get("demo::rvs_stale").is_none());
        assert!(target.rvs_get("std::fs::read_to_string").is_some());
        assert_eq!(target.rvs_len(), 2);
    }

    #[test]
    fn test_20260710_merge_std_like_callgraph_preserves_existing_node_merge() {
        let mut target_node = crate::artifacts::FnNode::default();
        target_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: crate::symbols::DefPath::from("core::rvs_target_call"),
            },
            CallEdgeType::Strong,
        );
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("std::rvs_shared".into(), target_node);

        let mut source_node = crate::artifacts::FnNode::default();
        source_node.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: crate::symbols::DefPath::from("alloc::rvs_source_call"),
            },
            CallEdgeType::Strong,
        );
        source_node.facts.has_async = true;
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M("std::rvs_shared".into(), source_node);
        source.rvs_insert_M(
            "demo::rvs_filtered".into(),
            crate::artifacts::FnNode::default(),
        );

        rvs_merge_std_like_callgraph_M(&mut target, &source).unwrap();
        let merged = target
            .rvs_get("std::rvs_shared")
            .expect("never: merged std node must exist");
        let calls = merged
            .calls
            .keys()
            .map(|identity| identity.def_path.rvs_as_str())
            .collect::<Vec<_>>()
            .join(",");
        let output = format!(
            "calls={calls}\nhas_async={}\nhas_filtered={}\nlen={}\n",
            merged.facts.has_async,
            target.rvs_get("demo::rvs_filtered").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260710_merge_std_like_callgraph_preserves_existing_node_merge",
            &output,
        );

        assert_eq!(merged.calls.len(), 2);
        assert!(merged.facts.has_async);
        assert!(target.rvs_get("demo::rvs_filtered").is_none());
        assert_eq!(target.rvs_len(), 1);
    }

    #[test]
    fn test_20260705_merge_std_like_callgraph_skips_local_std_crate_nodes() {
        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M("std::rvs_run".into(), crate::artifacts::FnNode::default());
        let mut source = FnGraph::rvs_new();
        source.rvs_insert_M("std::rvs_stale".into(), crate::artifacts::FnNode::default());
        source.rvs_insert_M(
            "core::mem::drop".into(),
            crate::artifacts::FnNode::default(),
        );
        let local_names = BTreeSet::from([CrateName::from("std")]);

        rvs_merge_std_like_callgraph_with_local_prefixes_M(&mut target, &source, &local_names)
            .unwrap();
        let output = format!(
            "has_local_stale={}\nhas_core={}\nlen={}\n",
            target.rvs_get("std::rvs_stale").is_some(),
            target.rvs_get("core::mem::drop").is_some(),
            target.rvs_len(),
        );
        rvs_snapshot_BIS(
            "test_20260705_merge_std_like_callgraph_skips_local_std_crate_nodes",
            &output,
        );

        assert!(target.rvs_get("std::rvs_stale").is_none());
        assert!(target.rvs_get("core::mem::drop").is_some());
        assert_eq!(target.rvs_len(), 2);
    }

    #[test]
    fn test_20260704_reject_stale_callgraph_without_has_body() {
        let result = crate::artifacts::rvs_parse_callgraph_json(
            r#"{
  "demo::rvs_trait_method_P": {
    "calls": [],
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        );
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260704_reject_stale_callgraph_without_has_body",
            &output,
        );

        assert!(matches!(
            result,
            Err(crate::artifacts::CallgraphArtifactError::StaleLegacyMissingHasBody {
                ref def_path
            }) if def_path.rvs_as_str() == "demo::rvs_trait_method_P"
        ));
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_rejects_empty_artifact_dir() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-callgraph-dir");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_rejects_empty_artifact_dir",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_merge_callgraph_dir_accepts_empty_graph_json() {
        let dir = rvs_make_workspace_temp_dir_BIS("empty-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        let json = crate::artifacts::rvs_serialize_callgraph_json(&FnGraph::rvs_new()).unwrap();
        std::fs::write(cg_dir.join("demo-1.json"), json).unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
        let output = format!(
            "is_ok={}\nis_empty={}\n",
            result.is_ok(),
            result.as_ref().is_ok_and(FnGraph::rvs_is_empty)
        );
        rvs_snapshot_BIS(
            "test_20260714_merge_callgraph_dir_accepts_empty_graph_json",
            &output,
        );

        assert!(result.is_ok());
        assert!(result.as_ref().is_ok_and(FnGraph::rvs_is_empty));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_sorts_json_artifacts() {
        let dir = rvs_make_workspace_temp_dir_BIS("sorted-callgraph-json");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(&cg_dir).unwrap();
        std::fs::write(cg_dir.join("z.json"), "not json\n").unwrap();
        std::fs::write(cg_dir.join("a.json"), "not json\n").unwrap();

        let result = rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::new());
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_sorts_json_artifacts",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("a.json"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_merge_callgraph_dir_ignores_json_directory() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-json-directory");
        let cg_dir = dir.join("target/rivus-callgraph");
        std::fs::create_dir_all(cg_dir.join("a.json")).unwrap();
        std::fs::write(
            cg_dir.join("b.json"),
            r#"{
  "demo::rvs_run": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result =
            rvs_merge_callgraph_dir_BIS(&cg_dir, &BTreeSet::from([CrateName::from("demo")]));
        let ok = result
            .as_ref()
            .is_ok_and(|graph| graph.rvs_get("demo::rvs_run").is_some());
        rvs_snapshot_BIS(
            "test_20260706_merge_callgraph_dir_ignores_json_directory",
            &format!("ok={ok}\n"),
        );

        assert!(ok);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260710_callgraph_artifact_write_failure_fails_cargo_BIS() {
        let dir = rvs_make_workspace_temp_dir_BIS("callgraph-artifact-write-failure");
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"callgraph-write-failure\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/lib.rs"),
            "pub fn rvs_value() -> u32 { 1 }\n",
        )
        .unwrap();
        let mut generation = rvs_reserve_run_generation_for_BIST(
            &project,
            RunGenerationMode::Collection {
                collection: RunGenerationCollectionMode::Workspace,
                target_scope: RunGenerationTargetScope::Production,
                lints: CollectionLints::Silent,
            },
        )
        .unwrap();
        let artifact_path = generation.rvs_artifact_dir().to_path_buf();
        std::fs::remove_dir(&artifact_path).unwrap();
        std::fs::write(&artifact_path, "blocker\n").unwrap();
        rvs_write_primary_package_targets_BIST(&project, &generation, CargoTargetScope::Production)
            .unwrap();

        let output = Command::new(rvs_cargo_command_from_env_BS())
            .arg("check")
            .arg("--quiet")
            .current_dir(&project)
            .env_remove("RUSTC")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("RIVUS_CALLGRAPH")
            .env_remove("RIVUS_CALLGRAPH_DIR")
            .env_remove("RIVUS_OFFLINE_CAPS")
            .env_remove("RIVUS_UI_TESTING")
            .env_remove("RIVUS_UNTESTED_PATHS")
            .env_remove("RIVUS_WRAPPER")
            .env_remove("RIVUS_GENERATION_ID")
            .env_remove("RIVUS_GENERATION_ROOT")
            .env("RUSTC_WRAPPER", rvs_current_wrapper_exe_BIS().unwrap())
            .env("RIVUS_ENABLED", "1")
            .env("RIVUS_WRAPPER", "1")
            .env("RIVUS_GENERATION_ID", generation.rvs_generation_id())
            .env("RIVUS_GENERATION_ROOT", generation.rvs_root())
            .env("RIVUS_CALLGRAPH", "1")
            .env("RIVUS_CRATE_PROVENANCE", "cargo-primary")
            .env("RIVUS_CALLGRAPH_DIR", &artifact_path)
            .env("CARGO_TARGET_DIR", dir.join("target"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let has_artifact_error = stderr.contains("cannot write rivus callgraph artifact");
        let has_old_warning = stderr.contains("warning: cannot write rivus callgraph artifact:");
        let mentions_artifact_path = stderr.contains(&artifact_path.to_string_lossy().into_owned());
        let snapshot = format!(
            "success={}\nhas_artifact_error={has_artifact_error}\nhas_old_warning={has_old_warning}\nmentions_artifact_path={mentions_artifact_path}\n",
            output.status.success()
        );

        assert!(!output.status.success(), "{snapshot}");
        assert!(has_artifact_error, "{snapshot}\nstderr:\n{stderr}");
        assert!(!has_old_warning, "{snapshot}");
        assert!(mentions_artifact_path, "{snapshot}");
        rvs_snapshot_BIS(
            "test_20260710_callgraph_artifact_write_failure_fails_cargo_BIS",
            &snapshot,
        );

        rvs_cleanup_run_generation_BIMS(&mut generation)
            .expect("never: failed-artifact generation cleanup should succeed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260809_driver_protocol_helpers_table() {
        use std::ffi::OsString as O;
        let one = O::from("1");
        let zero = O::from("0");
        let two = O::from("2");
        let path_str = O::from("/some/path");

        let require_flag = |v: Option<&O>| rvs_require_driver_flag(v, "X");
        let optional_flag = |v: Option<&O>| rvs_optional_driver_flag(v, "X");

        let require_utf8 = |v: Option<&O>| rvs_require_driver_utf8(v, "X");
        let require_path = |v: Option<&O>| rvs_require_driver_path(v, "X");
        let reject_var = |v: Option<&O>| rvs_reject_driver_variable(v, "X");
        let path_match =
            |v: Option<&O>| rvs_require_driver_path_match(v, "X", &PathBuf::from("/some/path"));

        let env_authority = DriverProtocolEnvironment {
            enabled: None,
            wrapper: None,
            generation_id: None,
            generation_root: None,
            callgraph: None,
            callgraph_dir: None,
            crate_provenance: None,
            capsmap: None,
            offline_caps: None,
            untested_paths: None,
            offline_emissions: None,
            offline_emissions_ack_dir: None,
            ui_testing: None,
            rustc_arguments: Vec::new(),
        };
        let mut env_with_wrapper = env_authority.clone();
        env_with_wrapper.wrapper = Some(O::from("anything"));

        let cases = [
            ("require_one_ok", require_flag(Some(&one)).is_ok()),
            ("require_zero_err", require_flag(Some(&zero)).is_err()),
            ("require_two_err", require_flag(Some(&two)).is_err()),
            ("require_missing_err", require_flag(None).is_err()),
            (
                "optional_one_true",
                optional_flag(Some(&one)).unwrap_or(false),
            ),
            ("optional_none_false", !optional_flag(None).unwrap_or(true)),
            ("optional_zero_err", optional_flag(Some(&zero)).is_err()),
            ("utf8_ok", require_utf8(Some(&path_str)).is_ok()),
            ("utf8_missing", require_utf8(None).is_err()),
            ("path_ok", require_path(Some(&path_str)).is_ok()),
            ("path_missing", require_path(None).is_err()),
            ("reject_present", reject_var(Some(&one)).is_err()),
            ("reject_absent", reject_var(None).is_ok()),
            ("path_match_ok", path_match(Some(&path_str)).is_ok()),
            (
                "path_match_mismatch",
                path_match(Some(&O::from("/other"))).is_err(),
            ),
            (
                "authority_empty",
                !env_authority.rvs_contains_rivus_authority(),
            ),
            (
                "authority_wrapper",
                env_with_wrapper.rvs_contains_rivus_authority(),
            ),
        ];

        let mut output = String::new();
        for (label, result) in cases {
            output.push_str(&format!("{label}={result}\n"));
        }
        rvs_snapshot_BIS("test_20260809_driver_protocol_helpers_table", &output);
    }

    #[test]
    fn test_20260809_merge_lint_results_priority() {
        let ok_ok = rvs_merge_lint_results(&Ok(()), &Ok(None));
        let ok_ack_err = rvs_merge_lint_results(&Ok(()), &Err("ack fail".into()));
        let cargo_err_ok = rvs_merge_lint_results(&Err(CargoCheckError::ExitCode(1)), &Ok(None));
        let both_err = rvs_merge_lint_results(
            &Err(CargoCheckError::Message("cargo fail".into())),
            &Err("ack fail".into()),
        );
        let output = format!(
            "ok_ok={}\nok_ack_err={}\ncargo_err_ok={}\nboth_err={}\n",
            ok_ok.is_ok(),
            ok_ack_err.is_err(),
            cargo_err_ok.is_err(),
            both_err.is_err(),
        );
        rvs_snapshot_BIS("test_20260809_merge_lint_results_priority", &output);

        assert!(ok_ok.is_ok());
        assert!(ok_ack_err.is_err_and(|e| matches!(
            e,
            CargoCheckError::Message(m) if m == "ack fail"
        )));
        assert!(cargo_err_ok.is_err_and(|e| matches!(e, CargoCheckError::ExitCode(1))));
        assert!(both_err.is_err_and(|e| matches!(
            e,
            CargoCheckError::Message(m) if m == "cargo fail"
        )));
    }

    #[test]
    fn test_20260831_check_collection_deny_short_circuits_graph_analysis() {
        let dir = rvs_make_cargo_project_BIS(
            "check-deny-short-circuit",
            "check-deny-short-circuit",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\npub fn rvs_pending() { todo!() }\n",
            )],
        );
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!("is_err={}\ncode={:?}\n", result.is_err(), result.err());
        rvs_snapshot_BIS(
            "test_20260831_check_collection_deny_short_circuits_graph_analysis",
            &output,
        );

        // The deny-level stub fires in the lint-bearing collection compile;
        // the command exits with the cargo code before graph analysis runs.
        assert_eq!(result, Err(101));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260831_check_warning_project_completes_full_pipeline() {
        let dir = rvs_make_cargo_project_BIS(
            "check-warning-pipeline",
            "check-warning-pipeline",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\npub fn rvs_add(left: i32, right: i32) -> i32 {\n    debug_assert!(left >= 0);\n    debug_assert!(right >= 0);\n    left + right\n}\n",
            )],
        );
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!("is_ok={}\n", result.is_ok());
        rvs_snapshot_BIS(
            "test_20260831_check_warning_project_completes_full_pipeline",
            &output,
        );

        // Warnings (missing doc, untested good fn) do not fail either phase.
        assert_eq!(result, Ok(()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260831_check_graph_error_fails_replay_phase() {
        let dir = rvs_make_cargo_project_BIS(
            "check-graph-error",
            "check-graph-error",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\nstatic FLAG: u32 = 7;\npub fn rvs_flag() -> u32 {\n    FLAG\n}\n",
            )],
        );
        // Phase isolation: the lint-bearing collection compile alone must
        // succeed for this fixture, so a failure of the full command can
        // only come from the replay phase.
        let target_scope = CargoTargetScope::WithTestExampleBench;
        let local_crate_names = rvs_load_local_crate_prefixes_BIS(&dir, target_scope)
            .expect("never: graph-error fixture should expose its local crate names");
        let collection_ok = rvs_collect_callgraph_with_args_detailed_BIST(
            &dir,
            CallgraphCollectionMode::Workspace,
            target_scope,
            vec![],
            &local_crate_names,
            CollectionLints::Check,
        )
        .is_ok();
        let result = rvs_run_cargo_check_at_BIST(&dir, &[]);
        let output = format!(
            "collection_ok={collection_ok}\nis_err={}\ncode={:?}\n",
            result.is_err(),
            result.err()
        );
        rvs_snapshot_BIS(
            "test_20260831_check_graph_error_fails_replay_phase",
            &output,
        );

        assert!(
            collection_ok,
            "fixture must not fail the collection compile"
        );
        // The contract mismatch is a graph Error replayed in the second
        // phase and fails the command with the cargo exit code.
        assert_eq!(result, Err(101));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
