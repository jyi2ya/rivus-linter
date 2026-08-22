use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use snafu::Snafu;

use crate::artifacts::{self, CallgraphArtifactError, FnGraph};
#[cfg(test)]
use crate::callgraph::{
    rvs_merge_std_like_callgraph_M, rvs_merge_std_like_callgraph_with_local_prefixes_M,
};
use crate::symbols::CrateName;

const STD_CALLGRAPH_CACHE_FILE: &str = "rivus-callgraph-std.json";
/// Legacy per-unit std callgraph directory kept for read-only diagnostics.
pub(crate) const STD_CALLGRAPH_CACHE_DIR: &str = "rivus-callgraph-std";

#[derive(Debug, Snafu)]
pub(crate) enum CallgraphCacheError {
    #[snafu(display("published std callgraph cache must be a regular file: {}", path.display()))]
    PublishedCacheNotFile { path: PathBuf },
    #[snafu(display("cannot read {}: {source}", path.display()))]
    ReadArtifact {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("{}: {source}", path.display()))]
    ParseArtifact {
        path: PathBuf,
        source: CallgraphArtifactError,
    },
    #[snafu(display(
        "published std callgraph cache {} is headerless legacy data; use the legacy directory only for read-only diagnostics",
        path.display()
    ))]
    PublishedCacheIsLegacy { path: PathBuf },
    #[snafu(display("cannot create {}: {source}", path.display()))]
    CreateCacheDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot serialize std callgraph cache: {source}"))]
    SerializePublishedCache { source: CallgraphArtifactError },
    #[snafu(display("cannot publish std callgraph cache: {message}"))]
    PublishCache { message: String },
    #[snafu(display("cannot read {}: {source}", path.display()))]
    ReadArtifactDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("readdir error in {}: {source}", path.display()))]
    ReadArtifactDirectoryEntry {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "callgraph artifact {} does not belong to generation {generation_id}",
        path.display()
    ))]
    ForeignGenerationArtifact {
        path: PathBuf,
        generation_id: String,
    },
    #[snafu(display("no callgraph JSON artifacts found in {}", path.display()))]
    NoArtifacts { path: PathBuf },
    #[snafu(display("cannot merge callgraph artifacts: {source}"))]
    MergeArtifacts { source: CallgraphArtifactError },
}

pub(crate) fn rvs_std_callgraph_cache_path(project_path: &Path) -> PathBuf {
    project_path.join("target").join(STD_CALLGRAPH_CACHE_FILE)
}

pub(crate) fn rvs_std_callgraph_cache_dir(project_path: &Path) -> PathBuf {
    project_path.join("target").join(STD_CALLGRAPH_CACHE_DIR)
}

pub(crate) fn rvs_load_published_std_callgraph_cache_BIS(
    project_path: &Path,
) -> Result<Option<FnGraph>, CallgraphCacheError> {
    rvs_load_published_std_callgraph_cache_with_hook_BIS(project_path, &|_| {})
}

fn rvs_load_published_std_callgraph_cache_with_hook_BIS(
    project_path: &Path,
    before_read: &impl Fn(&Path),
) -> Result<Option<FnGraph>, CallgraphCacheError> {
    let path = rvs_std_callgraph_cache_path(project_path);
    if !path.is_file() {
        return Ok(None);
    }
    before_read(&path);
    let json = super::fs_guard::rvs_read_file_utf8_BIS(&path).map_err(|source| {
        CallgraphCacheError::ReadArtifact {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let graph = artifacts::rvs_parse_callgraph_json(&json).map_err(|source| {
        CallgraphCacheError::ParseArtifact {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if graph.rvs_is_legacy() {
        return Err(CallgraphCacheError::PublishedCacheIsLegacy { path });
    }
    Ok(Some(graph))
}

pub(crate) fn rvs_publish_std_callgraph_cache_BIST(
    project_path: &Path,
    callgraph: &FnGraph,
) -> Result<(), CallgraphCacheError> {
    let path = rvs_std_callgraph_cache_path(project_path);
    let parent_path = path
        .parent()
        .expect("never: published cache path has a parent");
    std::fs::create_dir_all(parent_path).map_err(|source| {
        CallgraphCacheError::CreateCacheDirectory {
            path: parent_path.to_path_buf(),
            source,
        }
    })?;
    let json = artifacts::rvs_serialize_callgraph_json(callgraph)
        .map_err(|source| CallgraphCacheError::SerializePublishedCache { source })?;
    super::fs_guard::rvs_atomic_write_BIST(&path, json.as_bytes()).map_err(|error| {
        CallgraphCacheError::PublishCache {
            message: format!("cannot write {}: {error}", path.display()),
        }
    })
}

pub(crate) fn rvs_merge_callgraph_dir_BIS(
    cg_dir: &Path,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, CallgraphCacheError> {
    rvs_merge_callgraph_dir_for_generation_BIS(cg_dir, None, local_crate_names)
}

pub(crate) fn rvs_merge_generation_callgraph_dir_BIS(
    cg_dir: &Path,
    generation_id: &str,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, CallgraphCacheError> {
    rvs_merge_callgraph_dir_for_generation_BIS(cg_dir, Some(generation_id), local_crate_names)
}

fn rvs_merge_callgraph_dir_for_generation_BIS(
    cg_dir: &Path,
    generation_id: Option<&str>,
    local_crate_names: &BTreeSet<CrateName>,
) -> Result<FnGraph, CallgraphCacheError> {
    rvs_merge_callgraph_dir_for_generation_with_hook_BIS(
        cg_dir,
        generation_id,
        local_crate_names,
        &|_| {},
    )
}

fn rvs_merge_callgraph_dir_for_generation_with_hook_BIS(
    cg_dir: &Path,
    generation_id: Option<&str>,
    local_crate_names: &BTreeSet<CrateName>,
    before_read: &impl Fn(&Path),
) -> Result<FnGraph, CallgraphCacheError> {
    let mut artifacts = Vec::new();
    let mut json_paths = Vec::new();
    let entries =
        std::fs::read_dir(cg_dir).map_err(|source| CallgraphCacheError::ReadArtifactDirectory {
            path: cg_dir.to_path_buf(),
            source,
        })?;
    for entry in entries {
        let entry = entry.map_err(|source| CallgraphCacheError::ReadArtifactDirectoryEntry {
            path: cg_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| {
            CallgraphCacheError::ReadArtifactDirectoryEntry {
                path: cg_dir.to_path_buf(),
                source,
            }
        })?;
        if file_type.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Some(generation_id) = generation_id {
                let expected_prefix = format!("{generation_id}-");
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&expected_prefix))
                {
                    return Err(CallgraphCacheError::ForeignGenerationArtifact {
                        path,
                        generation_id: generation_id.to_string(),
                    });
                }
            }
            json_paths.push(path);
        }
    }
    json_paths.sort();
    for path in &json_paths {
        before_read(path);
        let json = super::fs_guard::rvs_read_file_utf8_BIS(path).map_err(|source| {
            CallgraphCacheError::ReadArtifact {
                path: path.to_path_buf(),
                source,
            }
        })?;
        artifacts.push(
            artifacts::rvs_parse_callgraph_json(&json).map_err(|source| {
                CallgraphCacheError::ParseArtifact {
                    path: path.to_path_buf(),
                    source,
                }
            })?,
        );
    }
    if json_paths.is_empty() {
        return Err(CallgraphCacheError::NoArtifacts {
            path: cg_dir.to_path_buf(),
        });
    }
    FnGraph::rvs_merge_artifacts(artifacts, local_crate_names)
        .map_err(|source| CallgraphCacheError::MergeArtifacts { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CALLGRAPH_SCHEMA_VERSION, CrateProvenance, FnNode};
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    fn rvs_current_std_graph() -> FnGraph {
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.crate_provenance = CrateProvenance::Dependency;
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("std::rvs_current"), node);
        graph
    }

    #[test]
    fn test_20260730_std_like_merge_errors_preserve_borrowed_source() {
        let source = crate::artifacts::rvs_parse_callgraph_json(
            r#"{"std::rvs_legacy":{"calls":[],"has_body":true}}"#,
        )
        .unwrap();
        let mut direct_target = rvs_current_std_graph();
        let direct = rvs_merge_std_like_callgraph_M(&mut direct_target, &source);
        let mut prefixed_target = rvs_current_std_graph();
        let prefixed = rvs_merge_std_like_callgraph_with_local_prefixes_M(
            &mut prefixed_target,
            &source,
            &BTreeSet::new(),
        );

        let direct_error = matches!(
            direct,
            Err(CallgraphArtifactError::MixedArtifactFormats { .. })
        );
        let prefixed_error = matches!(
            prefixed,
            Err(CallgraphArtifactError::MixedArtifactFormats { .. })
        );
        let source_retained = source.rvs_is_legacy() && source.rvs_get("std::rvs_legacy").is_some();
        let output = format!(
            "direct_error={direct_error}\nprefixed_error={prefixed_error}\nsource_retained={source_retained}\n"
        );
        rvs_snapshot_BIS(
            "test_20260730_std_like_merge_errors_preserve_borrowed_source",
            &output,
        );

        assert!(direct_error);
        assert!(prefixed_error);
        assert!(source_retained);
    }

    #[test]
    fn test_20260730_std_like_merge_is_transactional_for_optional_cache() {
        let conflict_path = DefPath::from("std::z_conflict");
        let mut target_conflict = FnNode::default();
        target_conflict.crate_id = 11;
        target_conflict.crate_provenance = CrateProvenance::Dependency;
        let mut source_conflict = target_conflict.clone();
        source_conflict.has_body = false;

        let mut target = FnGraph::rvs_new();
        target.rvs_insert_M(conflict_path.clone(), target_conflict);
        let mut source = FnGraph::rvs_new();
        let mut added = FnNode::default();
        added.crate_id = 12;
        added.crate_provenance = CrateProvenance::Dependency;
        source.rvs_insert_M(DefPath::from("std::a_added_before_conflict"), added);
        source.rvs_insert_M(conflict_path, source_conflict);

        let before = artifacts::rvs_serialize_callgraph_json(&target).unwrap();
        let direct_result = rvs_merge_std_like_callgraph_M(&mut target, &source);
        let direct_after = artifacts::rvs_serialize_callgraph_json(&target).unwrap();

        let mut optional_target = crate::artifacts::rvs_parse_callgraph_json(&before).unwrap();
        let optional_result = rvs_merge_std_like_callgraph_M(&mut optional_target, &source);
        let optional_after = artifacts::rvs_serialize_callgraph_json(&optional_target).unwrap();
        let output = format!(
            "direct_ok={}\ndirect_changed={}\ndirect_added_node_present={}\noptional_ok={}\noptional_changed={}\noptional_added_node_present={}\n",
            direct_result.is_ok(),
            direct_after != before,
            target.rvs_get("std::a_added_before_conflict").is_some(),
            optional_result.is_ok(),
            optional_after != before,
            optional_target
                .rvs_get("std::a_added_before_conflict")
                .is_some(),
        );
        rvs_snapshot_BIS(
            "test_20260730_std_like_merge_is_transactional_for_optional_cache",
            &output,
        );

        assert!(direct_result.is_ok());
        assert_ne!(direct_after, before);
        assert!(target.rvs_get("std::a_added_before_conflict").is_some());
        assert!(optional_result.is_ok());
        assert_ne!(optional_after, before);
        assert!(
            optional_target
                .rvs_get("std::a_added_before_conflict")
                .is_some()
        );
    }

    #[test]
    fn test_20260729_published_cache_errors_remain_structured() {
        let dir = rvs_make_temp_dir_BIS("published-callgraph-structured-errors");
        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        let cache = rvs_std_callgraph_cache_path(&dir);

        std::fs::write(
            &cache,
            r#"{"std::rvs_legacy":{"calls":[],"has_body":true}}"#,
        )
        .unwrap();
        let legacy = rvs_load_published_std_callgraph_cache_BIS(&dir);
        let legacy_variant = matches!(
            legacy,
            Err(CallgraphCacheError::PublishedCacheIsLegacy { .. })
        );

        std::fs::write(&cache, r#"{"schema_version":12,"nodes":{"std::broken""#).unwrap();
        let truncated = rvs_load_published_std_callgraph_cache_BIS(&dir);
        let truncated_variant = matches!(
            truncated,
            Err(CallgraphCacheError::ParseArtifact {
                source: CallgraphArtifactError::InvalidJson { .. },
                ..
            })
        );

        std::fs::write(&cache, r#"{"schema_version":11,"nodes":{}}"#).unwrap();
        let incompatible = rvs_load_published_std_callgraph_cache_BIS(&dir);
        let incompatible_variant = matches!(
            incompatible,
            Err(CallgraphCacheError::ParseArtifact {
                source: CallgraphArtifactError::UnsupportedSchemaVersion {
                    actual: 11,
                    expected: CALLGRAPH_SCHEMA_VERSION,
                },
                ..
            })
        );
        let output = format!(
            "legacy_variant={legacy_variant}\ntruncated_variant={truncated_variant}\nincompatible_variant={incompatible_variant}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_published_cache_errors_remain_structured",
            &output,
        );

        assert!(legacy_variant);
        assert!(truncated_variant);
        assert!(incompatible_variant);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_cache_mergers_reject_legacy_current_mixtures() {
        let dir = rvs_make_temp_dir_BIS("mixed-callgraph-cache-formats");
        let artifacts = dir.join("artifacts");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(
            artifacts.join("current.json"),
            crate::artifacts::rvs_serialize_callgraph_json(&rvs_current_std_graph()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            artifacts.join("legacy.json"),
            r#"{"std::rvs_legacy":{"calls":[],"has_body":true}}"#,
        )
        .unwrap();
        let directory = rvs_merge_callgraph_dir_BIS(&artifacts, &BTreeSet::new());
        let directory_variant = matches!(
            directory,
            Err(CallgraphCacheError::MergeArtifacts {
                source: CallgraphArtifactError::MixedArtifactFormats { .. }
            })
        );

        let mut current = rvs_current_std_graph();
        let legacy = crate::artifacts::rvs_parse_callgraph_json(
            r#"{"std::rvs_legacy":{"calls":[],"has_body":true}}"#,
        )
        .unwrap();
        let direct = rvs_merge_std_like_callgraph_M(&mut current, &legacy);
        let direct_variant = matches!(
            direct,
            Err(CallgraphArtifactError::MixedArtifactFormats { .. })
        );

        let empty_legacy = crate::artifacts::rvs_parse_callgraph_json(r#"{}"#).unwrap();
        let empty_direct =
            rvs_merge_std_like_callgraph_M(&mut rvs_current_std_graph(), &empty_legacy);
        let empty_direct_variant = matches!(
            empty_direct,
            Err(CallgraphArtifactError::MixedArtifactFormats { .. })
        );
        let output = format!(
            "directory_variant={directory_variant}\ndirect_variant={direct_variant}\nempty_direct_variant={empty_direct_variant}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_cache_mergers_reject_legacy_current_mixtures",
            &output,
        );

        assert!(directory_variant);
        assert!(direct_variant);
        assert!(empty_direct_variant);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_generation_merge_rejects_foreign_artifacts() {
        let dir = rvs_make_temp_dir_BIS("foreign-generation-artifact");
        let artifacts = dir.join("artifacts");
        std::fs::create_dir(&artifacts).unwrap();
        let json =
            crate::artifacts::rvs_serialize_callgraph_json(&rvs_current_std_graph()).unwrap();
        std::fs::write(artifacts.join("rivus-v4-foreign-demo-1-0.json"), &json).unwrap();
        let foreign =
            rvs_merge_generation_callgraph_dir_BIS(&artifacts, "rivus-v4-owned", &BTreeSet::new());
        let foreign_rejected = matches!(
            foreign,
            Err(CallgraphCacheError::ForeignGenerationArtifact { .. })
        );
        std::fs::remove_file(artifacts.join("rivus-v4-foreign-demo-1-0.json")).unwrap();
        std::fs::write(artifacts.join("rivus-v4-owned-demo-1-0.json"), json).unwrap();
        let owned_accepted =
            rvs_merge_generation_callgraph_dir_BIS(&artifacts, "rivus-v4-owned", &BTreeSet::new())
                .is_ok();
        let output =
            format!("foreign_rejected={foreign_rejected}\nowned_accepted={owned_accepted}\n");
        rvs_snapshot_BIS(
            "test_20260729_generation_merge_rejects_foreign_artifacts",
            &output,
        );

        assert!(foreign_rejected);
        assert!(owned_accepted);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
