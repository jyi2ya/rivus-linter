use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use snafu::Snafu;

use crate::artifacts::{CrateProvenance, FnGraph, FunctionIdentity};
use crate::capsmap::{self, CapsMap};
use crate::lints::{LintEnvironment, RivusLintConfig};
use crate::offline_caps::OfflineCapsEmission;
use crate::symbols::CrateName;

use super::workspace::{
    RivusCallgraphOutput, RivusDriverConfig, RivusDriverMode, RivusOfflineDriverInput,
};

static RVS_CALLGRAPH_ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Snafu)]
enum CallgraphWriteError {
    #[snafu(display(
        "artifact generation identity must be a non-empty path-safe segment: {value}"
    ))]
    InvalidGenerationIdentity { value: String },
    #[snafu(display("artifact crate name must be a non-empty path segment: {crate_name}"))]
    InvalidCrateName { crate_name: CrateName },
    #[snafu(display("cannot serialize callgraph artifact: {source}"))]
    Serialize {
        source: crate::artifacts::CallgraphArtifactError,
    },
    #[snafu(display("cannot write callgraph artifact: {message}"))]
    WriteArtifact { message: String },
    #[snafu(display("cannot allocate a unique artifact path in {}", path.display()))]
    PathExhausted { path: PathBuf },
}

#[derive(Debug)]
pub(crate) struct RivusLintEnvironment;

#[derive(Debug)]
pub(crate) struct RivusLintWorld {
    callgraph_output: Option<RivusCallgraphOutput>,
    acknowledgement_dir: Option<PathBuf>,
}

impl LintEnvironment for RivusLintEnvironment {
    type World = RivusLintWorld;

    fn rvs_write_callgraph_BIMPST(
        world: &mut Self::World,
        crate_name: &CrateName,
        callgraph: &FnGraph,
    ) -> Result<(), String> {
        let Some(output) = &world.callgraph_output else {
            return Ok(());
        };
        rvs_write_bound_callgraph_artifact_BIST(output, crate_name, callgraph)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn rvs_acknowledge_offline_emission_BIMPS(
        world: &mut Self::World,
        emission_index: usize,
        anchor_index: usize,
    ) -> Result<(), String> {
        debug_assert!(
            emission_index < usize::MAX,
            "emission index must leave room for bounded acknowledgement arithmetic"
        );
        debug_assert!(
            anchor_index < usize::MAX,
            "anchor index must leave room for bounded acknowledgement arithmetic"
        );
        let Some(directory) = &world.acknowledgement_dir else {
            return Ok(());
        };
        let path = directory.join(crate::offline_caps::rvs_emission_ack_name(
            emission_index,
            anchor_index,
        ));
        std::fs::write(&path, []).map_err(|error| {
            format!(
                "cannot acknowledge offline caps diagnostic {}: {error}",
                path.display()
            )
        })
    }
}

pub(crate) fn rvs_prepare_lint_config_BIS(
    driver_config: RivusDriverConfig,
) -> RivusLintConfig<RivusLintEnvironment> {
    let RivusDriverConfig { mode, ui_testing } = driver_config;
    let (capsmap_path, offline_input, callgraph_output) = match mode {
        RivusDriverMode::ProjectCaps { capsmap } => (capsmap, None, None),
        RivusDriverMode::Offline(input) => (None, Some(input), None),
        RivusDriverMode::Callgraph(output) => (None, None, Some(output)),
    };
    let collect_callgraph = callgraph_output.is_some();
    let should_emit_caps_report = !collect_callgraph && offline_input.is_none();
    let capsmap = if should_emit_caps_report {
        rvs_load_capsmap_path_BIS(capsmap_path.as_deref()).map(Some)
    } else {
        Ok(None)
    };
    let untested_functions = rvs_load_untested_functions_BIS(offline_input.as_ref());
    let offline_emissions = rvs_load_offline_emissions_BIS(offline_input.as_ref());
    let acknowledgement_dir = offline_input.and_then(|input| input.acknowledgement_dir);
    let crate_provenance = callgraph_output
        .as_ref()
        .map_or(CrateProvenance::LegacyUnknown, |output| {
            output.crate_provenance
        });

    RivusLintConfig {
        capsmap,
        untested_functions,
        offline_emissions,
        test_outputs: rvs_collect_test_outputs_BIS(Path::new("test_out"), ui_testing),
        collect_callgraph,
        should_emit_caps_report,
        ui_testing,
        crate_provenance,
        world: RivusLintWorld {
            callgraph_output,
            acknowledgement_dir,
        },
        interpreter: std::marker::PhantomData,
    }
}

pub(crate) fn rvs_load_capsmap_path_BIS(path: Option<&Path>) -> Result<CapsMap, String> {
    match path {
        Some(path) => CapsMap::rvs_load_effective_BIS(path).map_err(|error| error.to_string()),
        None => capsmap::rvs_load_distributed_seed().map_err(|error| error.to_string()),
    }
}

fn rvs_load_untested_functions_BIS(
    input: Option<&RivusOfflineDriverInput>,
) -> Result<Option<BTreeSet<FunctionIdentity>>, String> {
    let Some(path) = input.and_then(|input| input.untested_paths.as_deref()) else {
        return Ok(None);
    };
    let json = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read untested-function selection {}: {error}",
            path.display()
        )
    })?;
    crate::artifacts::rvs_parse_function_identities_json_S(&json)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn rvs_load_offline_emissions_BIS(
    input: Option<&RivusOfflineDriverInput>,
) -> Result<Vec<OfflineCapsEmission>, String> {
    let Some(path) = input.and_then(|input| input.emissions.as_deref()) else {
        return Ok(Vec::new());
    };
    let json = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read offline caps emissions {}: {error}",
            path.display()
        )
    })?;
    crate::offline_caps::rvs_parse_emissions(&json)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn rvs_collect_test_outputs_BIS(directory: &Path, ui_testing: bool) -> Option<BTreeSet<String>> {
    if ui_testing || !directory.is_dir() {
        return None;
    }
    let mut outputs = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Some(outputs);
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(test_name) = name.strip_suffix(".out") {
            outputs.insert(test_name.to_string());
        }
    }
    Some(outputs)
}

fn rvs_write_bound_callgraph_artifact_BIST(
    output: &RivusCallgraphOutput,
    crate_name: &CrateName,
    callgraph: &FnGraph,
) -> Result<PathBuf, CallgraphWriteError> {
    let generation_id = output.generation_id.as_str();
    let json = rvs_serialize_callgraph_artifact_S(generation_id, crate_name, callgraph)?;
    for _ in 0..100usize {
        let sequence = RVS_CALLGRAPH_ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let file_name = format!(
            "{generation_id}-{crate_name}-{}-{sequence}.json",
            std::process::id()
        );
        match output.rvs_write_artifact_file_no_replace_BIST(&file_name, json.as_bytes()) {
            Ok(()) => return Ok(output.artifact_dir.join(file_name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CallgraphWriteError::WriteArtifact {
                    message: error.to_string(),
                });
            }
        }
    }
    Err(CallgraphWriteError::PathExhausted {
        path: output.artifact_dir.clone(),
    })
}

fn rvs_serialize_callgraph_artifact_S(
    generation_id: &str,
    crate_name: &CrateName,
    callgraph: &FnGraph,
) -> Result<String, CallgraphWriteError> {
    if generation_id.is_empty()
        || !generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(CallgraphWriteError::InvalidGenerationIdentity {
            value: generation_id.to_string(),
        });
    }
    let crate_name = crate_name.rvs_as_str();
    if crate_name.is_empty()
        || crate_name.contains('/')
        || crate_name.contains('\\')
        || crate_name.contains('\0')
    {
        return Err(CallgraphWriteError::InvalidCrateName {
            crate_name: CrateName::from(crate_name),
        });
    }
    crate::artifacts::rvs_serialize_callgraph_json_S(callgraph)
        .map_err(|source| CallgraphWriteError::Serialize { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FnNode;
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    const TEST_GENERATION_ID: &str = "rivus-v4-test-generation";

    fn rvs_test_callgraph() -> FnGraph {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.rvs_test_capture_target_M(1, true, true);
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), node);
        graph
    }

    fn rvs_test_callgraph_output_BIS(directory: &Path) -> RivusCallgraphOutput {
        RivusCallgraphOutput::rvs_for_test_BIS(TEST_GENERATION_ID, directory)
            .expect("never: test artifact directory should bind")
    }

    #[test]
    fn test_20260703_has_test_output_false_when_dir_missing() {
        let directory = rvs_make_temp_dir_BIS("test-output-missing");
        std::fs::remove_dir_all(&directory)
            .expect("never: missing test-output fixture should be removable");

        let outputs = rvs_collect_test_outputs_BIS(&directory, false);
        let exists = outputs.as_ref().is_some_and(|outputs| {
            outputs.contains("test_20260703_has_test_output_false_when_dir_missing")
        });
        rvs_snapshot_BIS(
            "test_20260703_has_test_output_false_when_dir_missing",
            &format!("exists={exists}\n"),
        );

        assert!(!exists);
        assert!(outputs.is_none());
    }

    #[test]
    fn test_20260714_missing_test_out_disables_snapshot_lint() {
        let directory = rvs_make_temp_dir_BIS("test-output-disabled");
        std::fs::remove_dir_all(&directory)
            .expect("never: disabled test-output fixture should be removable");

        let enabled = rvs_collect_test_outputs_BIS(&directory, false).is_some();
        rvs_snapshot_BIS(
            "test_20260714_missing_test_out_disables_snapshot_lint",
            &format!("enabled={enabled}\n"),
        );

        assert!(!enabled);
    }

    #[test]
    fn test_20260703_has_test_output_true_for_existing_snapshot() {
        let directory = rvs_make_temp_dir_BIS("test-output-existing");
        std::fs::write(
            directory.join("test_20260703_has_test_output_true_for_existing_snapshot.out"),
            "ok\n",
        )
        .expect("never: test-output fixture should be writable");

        let outputs = rvs_collect_test_outputs_BIS(&directory, false)
            .expect("never: existing test-output directory should enable checks");
        let exists = outputs.contains("test_20260703_has_test_output_true_for_existing_snapshot");
        rvs_snapshot_BIS(
            "test_20260703_has_test_output_true_for_existing_snapshot",
            &format!("exists={exists}\n"),
        );

        assert!(exists);
        std::fs::remove_dir_all(directory)
            .expect("never: test-output fixture cleanup should succeed");
    }

    #[test]
    fn test_20260706_has_test_output_false_for_snapshot_directory() {
        let directory = rvs_make_temp_dir_BIS("test-output-directory");
        std::fs::create_dir_all(
            directory.join("test_20260706_has_test_output_false_for_snapshot_directory.out"),
        )
        .expect("never: snapshot directory fixture should be creatable");

        let outputs = rvs_collect_test_outputs_BIS(&directory, false)
            .expect("never: existing test-output directory should enable checks");
        let exists = outputs.contains("test_20260706_has_test_output_false_for_snapshot_directory");
        rvs_snapshot_BIS(
            "test_20260706_has_test_output_false_for_snapshot_directory",
            &format!("exists={exists}\n"),
        );

        assert!(!exists);
        std::fs::remove_dir_all(directory)
            .expect("never: snapshot directory fixture cleanup should succeed");
    }

    #[test]
    fn test_20260714_explicit_capsmap_load_failure_is_fatal() {
        let directory = rvs_make_temp_dir_BIS("invalid-explicit-capsmap");
        std::fs::write(directory.join("seed"), "invalid capsmap line\n")
            .expect("never: invalid capsmap fixture should be writable");

        let result = rvs_load_capsmap_path_BIS(Some(&directory));
        let output = match &result {
            Ok(_) => "ok\n".to_string(),
            Err(error) => format!("error={error}\n")
                .replace(directory.to_string_lossy().as_ref(), "CAPSMAP_PATH"),
        };
        rvs_snapshot_BIS(
            "test_20260714_explicit_capsmap_load_failure_is_fatal",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(directory)
            .expect("never: invalid capsmap fixture cleanup should succeed");
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260706_write_json_artifact_uses_final_json_file() {
        let directory = rvs_make_temp_dir_BIS("artifact-write");
        let output = rvs_test_callgraph_output_BIS(&directory);
        let path = rvs_write_bound_callgraph_artifact_BIST(
            &output,
            &CrateName::from("demo"),
            &rvs_test_callgraph(),
        )
        .expect("never: artifact write should succeed");
        let tmp_exists = std::fs::read_dir(&directory)
            .expect("never: artifact directory should be readable")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"));
        let actual_file_name = path
            .file_name()
            .expect("never: artifact path has a file name")
            .to_string_lossy();
        let expected_prefix = format!("{TEST_GENERATION_ID}-demo-{}-", std::process::id());
        let sequence = actual_file_name
            .strip_prefix(&expected_prefix)
            .and_then(|suffix| suffix.strip_suffix(".json"));
        let file_name = if sequence.is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            format!("{TEST_GENERATION_ID}-demo-$PID-$SEQ.json")
        } else {
            actual_file_name.into_owned()
        };
        let content = std::fs::read_to_string(&path)
            .expect("never: written callgraph artifact should be readable");
        let snapshot = format!("file={file_name}\ncontent={content}\ntmp_exists={tmp_exists}\n",);
        rvs_snapshot_BIS(
            "test_20260706_write_json_artifact_uses_final_json_file",
            &snapshot,
        );

        assert!(path.is_file());
        assert!(!tmp_exists);
        std::fs::remove_dir_all(directory).expect("never: artifact fixture cleanup should succeed");
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260729_same_process_callgraph_writes_do_not_replace_artifacts() {
        let directory = rvs_make_temp_dir_BIS("artifact-same-process");
        let output = rvs_test_callgraph_output_BIS(&directory);
        let graph = rvs_test_callgraph();

        let first =
            rvs_write_bound_callgraph_artifact_BIST(&output, &CrateName::from("demo"), &graph)
                .expect("never: first artifact write should succeed");
        let second =
            rvs_write_bound_callgraph_artifact_BIST(&output, &CrateName::from("demo"), &graph)
                .expect("never: second artifact write should succeed");
        let artifact_count = std::fs::read_dir(&directory)
            .expect("never: artifact directory should be readable")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .count();
        let paths_distinct = first != second;
        let snapshot =
            format!("paths_distinct={paths_distinct}\nartifact_count={artifact_count}\n");
        rvs_snapshot_BIS(
            "test_20260729_same_process_callgraph_writes_do_not_replace_artifacts",
            &snapshot,
        );

        assert!(paths_distinct);
        assert_eq!(artifact_count, 2);
        std::fs::remove_dir_all(directory)
            .expect("never: same-process artifact fixture cleanup should succeed");
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260714_write_empty_callgraph_artifact() {
        let directory = rvs_make_temp_dir_BIS("artifact-empty-json");
        let output = rvs_test_callgraph_output_BIS(&directory);
        let result = rvs_write_bound_callgraph_artifact_BIST(
            &output,
            &CrateName::from("demo"),
            &FnGraph::rvs_new(),
        );
        let dir_exists = directory.exists();
        let content = result
            .as_ref()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        let snapshot = format!(
            "is_ok={}\ndir_exists={dir_exists}\ncontent={content}\n",
            result.is_ok()
        );
        rvs_snapshot_BIS("test_20260714_write_empty_callgraph_artifact", &snapshot);

        assert!(result.is_ok());
        assert!(dir_exists);
        assert!(content.contains(r#""nodes":{}"#));
        std::fs::remove_dir_all(directory)
            .expect("never: empty artifact fixture cleanup should succeed");
    }

    #[test]
    fn test_20260707_write_json_artifact_rejects_pathy_crate_name() {
        let directory = rvs_make_temp_dir_BIS("artifact-pathy-crate");
        std::fs::remove_dir_all(&directory)
            .expect("never: pathy crate fixture directory should be removable");
        let graph = rvs_test_callgraph();

        let slash = rvs_serialize_callgraph_artifact_S(
            TEST_GENERATION_ID,
            &CrateName::from("bad/name"),
            &graph,
        );
        let empty =
            rvs_serialize_callgraph_artifact_S(TEST_GENERATION_ID, &CrateName::from(""), &graph);
        let dir_exists = directory.exists();
        let snapshot = format!(
            "slash_is_err={}\nempty_is_err={}\ndir_exists={dir_exists}\n",
            slash.is_err(),
            empty.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260707_write_json_artifact_rejects_pathy_crate_name",
            &snapshot,
        );

        assert!(slash.is_err());
        assert!(empty.is_err());
        assert!(!dir_exists);
    }

    #[test]
    fn test_20260707_write_json_artifact_rejects_nul_crate_name() {
        let directory = rvs_make_temp_dir_BIS("artifact-nul-crate");
        std::fs::remove_dir_all(&directory)
            .expect("never: nul crate fixture directory should be removable");
        let result = rvs_serialize_callgraph_artifact_S(
            TEST_GENERATION_ID,
            &CrateName::from("bad\0name"),
            &rvs_test_callgraph(),
        );
        let dir_exists = directory.exists();
        let snapshot = format!("is_err={}\ndir_exists={dir_exists}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260707_write_json_artifact_rejects_nul_crate_name",
            &snapshot,
        );

        assert!(result.is_err());
        assert!(!dir_exists);
    }
}
