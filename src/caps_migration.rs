use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use snafu::Snafu;

use crate::capability::{
    CapabilityCompleteness, CapabilityInfo, CapabilityParseError, CapabilitySet,
};
use crate::capsmap::{CapsMap, CapsMapError, rvs_sort_by_layer_M};
use crate::symbols::CapsMapKey;

#[derive(Debug, Snafu)]
enum LegacyCapsError {
    #[snafu(display("line {line}: caps layer already uses the v2 header"))]
    AlreadyV2 { line: usize },
    #[snafu(display("line {line}: invalid capability string '{caps}' for '{key}'"))]
    InvalidCaps {
        key: CapsMapKey,
        caps: String,
        line: usize,
        source: CapabilityParseError,
    },
    #[snafu(display("line {line}: missing '=' separator"))]
    MissingSeparator { line: usize },
    #[snafu(display("line {line}: empty capsmap key"))]
    EmptyKey { line: usize },
    #[snafu(display(
        "line {line}: duplicate capsmap key '{key}' (first defined on line {first_line})"
    ))]
    DuplicateKey {
        key: CapsMapKey,
        first_line: usize,
        line: usize,
    },
}

#[derive(Debug, Snafu)]
enum CapsMigrationError {
    #[snafu(display("{message}"))]
    Project { message: String },
    #[snafu(display("cannot inspect {}: {source}", path.display()))]
    Inspect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("caps path must be a real directory, not a symlink: {}", path.display()))]
    CapsSymlink { path: PathBuf },
    #[snafu(display("caps path must be a directory: {}", path.display()))]
    CapsNotDirectory { path: PathBuf },
    #[snafu(display("caps v1 backup already exists: {}", path.display()))]
    BackupExists { path: PathBuf },
    #[snafu(display("another caps migration is already running for {}", path.display()))]
    MigrationInProgress { path: PathBuf },
    #[snafu(display("cannot lock caps migration directory {}: {source}", path.display()))]
    LockAcquire {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot read caps directory {}: {source}", path.display()))]
    ReadDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("caps layer must be a regular file: {}", path.display()))]
    LayerNotFile { path: PathBuf },
    #[snafu(display("cannot read caps layer {}: {source}", path.display()))]
    ReadLayer {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot migrate caps layer {}: {source}", path.display()))]
    ParseLayer {
        path: PathBuf,
        source: LegacyCapsError,
    },
    #[snafu(display("cannot create caps migration staging directory: too many collisions"))]
    StageCreateExhausted,
    #[snafu(display("cannot create caps migration staging directory {}: {source}", path.display()))]
    StageCreate {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("caps migration staging failed: {message}{cleanup}"))]
    StageOperation { message: String, cleanup: String },
    #[snafu(display("cannot sync staged caps directory {}: {source}", path.display()))]
    StageSync {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "cannot sync legacy caps directory {} before finalizing migration: {source}",
        path.display()
    ))]
    LegacySync {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("migrated caps validation failed: {source}"))]
    Validate { source: CapsMapError },
    #[snafu(display("migrated caps do not preserve the effective v1 capability map"))]
    SemanticMismatch,
    #[snafu(display("migrated caps layer '{layer}' does not preserve its v1 capability map"))]
    LayerSemanticMismatch { layer: String },
    #[snafu(display(
        "cannot recover interrupted caps migration: multiple v1 staging directories remain beside {}",
        path.display()
    ))]
    AmbiguousRecovery { path: PathBuf },
    #[snafu(display("invalid interrupted caps migration marker: {message}"))]
    InvalidRecoveryMarker { message: String },
    #[snafu(display(
        "cannot recover interrupted caps migration because active v2 directory {} is invalid: {source}",
        path.display()
    ))]
    RecoveryActiveInvalid { path: PathBuf, source: CapsMapError },
    #[snafu(display(
        "interrupted v1 staging directory {} does not match the active v2 caps layers",
        staging.display()
    ))]
    RecoveryMismatch { staging: PathBuf },
    #[snafu(display(
        "cannot recover interrupted caps migration: both staging {} and backup {} exist",
        staging.display(),
        backup.display()
    ))]
    RecoverySourcesConflict { staging: PathBuf, backup: PathBuf },
    #[snafu(display(
        "cannot recover interrupted caps migration: neither staging {} nor backup {} exists",
        staging.display(),
        backup.display()
    ))]
    RecoverySourceMissing { staging: PathBuf, backup: PathBuf },
    #[snafu(display(
        "caps migration source contains an ignored temporary-shaped entry: {}",
        path.display()
    ))]
    TemporarySourceEntry { path: PathBuf },
    #[snafu(display(
        "cannot preserve recovered v1 staging directory {} as backup {}: {source}",
        staging.display(),
        backup.display()
    ))]
    RecoveryPublish {
        staging: PathBuf,
        backup: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot remove caps migration marker {}: {source}", path.display()))]
    MarkerRemove {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "cannot atomically exchange original caps directory {} with staged directory {}: {source}{cleanup}",
        caps.display(),
        staging.display()
    ))]
    PublishExchange {
        caps: PathBuf,
        staging: PathBuf,
        source: std::io::Error,
        cleanup: String,
    },
    #[snafu(display(
        "exchanged-out original caps directory {} no longer matches published v2 caps: {message}; rollback: {rollback}",
        staging.display()
    ))]
    PublishValidation {
        staging: PathBuf,
        message: String,
        rollback: String,
    },
    #[snafu(display(
        "cannot preserve original caps directory {} as backup {}: {source}; rollback: {rollback}",
        staging.display(),
        backup.display()
    ))]
    BackupPublish {
        staging: PathBuf,
        backup: PathBuf,
        source: std::io::Error,
        rollback: String,
    },
    #[snafu(display(
        "caps migration was published but cannot sync parent directory {}: {source}",
        path.display()
    ))]
    PublishSync {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "caps migration was published but cannot make marker removal durable in {}: {source}; {recovery}",
        path.display()
    ))]
    MarkerFinalizeSync {
        path: PathBuf,
        source: std::io::Error,
        recovery: String,
    },
}

#[derive(Debug)]
struct ConvertedLayer {
    name: OsString,
    map: CapsMap,
    rendered: String,
    permissions: std::fs::Permissions,
}

#[derive(Debug)]
struct MigrationMarker {
    path: PathBuf,
    content: String,
}

pub(crate) fn rvs_run_migrate_caps_BIS(path: &Path) -> Result<(), String> {
    crate::workspace::rvs_ensure_cargo_project_BIS(path)?;
    let project = path
        .canonicalize()
        .map_err(|source| CapsMigrationError::Inspect {
            path: path.to_path_buf(),
            source,
        })
        .map_err(|error| error.to_string())?;
    let caps = project.join("caps");
    let backup = project.join("caps.v1-backup");
    rvs_migrate_caps_dir_BIS(&caps, &backup).map_err(|error| error.to_string())?;
    println!(
        "Migrated {} to capsmap v2; original preserved at {}",
        caps.display(),
        backup.display()
    );
    Ok(())
}

fn rvs_migrate_caps_dir_BIS(caps: &Path, backup: &Path) -> Result<(), CapsMigrationError> {
    let _lock = rvs_lock_caps_migration_BIS(caps)?;
    let caps_permissions = rvs_require_real_caps_dir_BIS(caps)?;
    if rvs_recover_interrupted_publish_BIS(caps, backup)? {
        return Ok(());
    }
    if rvs_path_exists_BIS(backup)? {
        return Err(CapsMigrationError::BackupExists {
            path: backup.to_path_buf(),
        });
    }

    let converted = rvs_collect_converted_layers_BIS(caps)?;
    let mut legacy_effective = CapsMap::rvs_new();
    for layer in &converted {
        legacy_effective.rvs_extend_from_M(layer.map.clone());
    }

    let staging = rvs_create_staging_dir_BIS(caps)?;
    for layer in &converted {
        let output = staging.join(&layer.name);
        if let Err(message) = crate::workspace::rvs_write_capsmap_result_BIS(
            &layer.rendered,
            &output,
            "migrated caps layer",
        ) {
            return Err(rvs_stage_operation_error_BIS(&staging, message));
        }
        if let Err(source) =
            crate::fs_guard::rvs_set_permissions_no_follow_BIS(&output, layer.permissions.clone())
        {
            return Err(rvs_stage_operation_error_BIS(
                &staging,
                format!(
                    "cannot preserve permissions for migrated caps layer {}: {source}",
                    output.display()
                ),
            ));
        }
    }
    if let Err(error) = rvs_validate_staged_layers_BIS(&staging, &converted) {
        return Err(rvs_stage_operation_error_BIS(&staging, error.to_string()));
    }
    if let Err(error) = rvs_write_migration_marker_BIS(&staging) {
        return Err(rvs_stage_operation_error_BIS(&staging, error));
    }
    if let Err(source) =
        crate::fs_guard::rvs_set_permissions_no_follow_BIS(&staging, caps_permissions)
    {
        return Err(rvs_stage_operation_error_BIS(
            &staging,
            format!(
                "cannot preserve permissions for migrated caps directory {}: {source}",
                staging.display()
            ),
        ));
    }

    let staged = match CapsMap::rvs_load_dir_BIS(&staging) {
        Ok(staged) => staged,
        Err(source) => {
            let error = CapsMigrationError::Validate { source };
            return Err(rvs_stage_operation_error_BIS(&staging, error.to_string()));
        }
    };
    if rvs_semantic_caps(&legacy_effective) != rvs_semantic_caps(&staged) {
        let error = CapsMigrationError::SemanticMismatch;
        return Err(rvs_stage_operation_error_BIS(&staging, error.to_string()));
    }
    if let Err(source) = rvs_sync_caps_dir_BIS(&staging) {
        let error = CapsMigrationError::StageSync {
            path: staging.clone(),
            source,
        };
        return Err(rvs_stage_operation_error_BIS(&staging, error.to_string()));
    }
    let current = match rvs_collect_converted_layers_BIS(caps) {
        Ok(current) => current,
        Err(error) => return Err(rvs_stage_operation_error_BIS(&staging, error.to_string())),
    };
    if rvs_semantic_layers(&current) != rvs_semantic_layers(&converted) {
        return Err(rvs_stage_operation_error_BIS(
            &staging,
            "caps layers changed while migration was preparing the v2 staging directory"
                .to_string(),
        ));
    }

    rvs_publish_staged_caps_BIS(
        caps,
        backup,
        &staging,
        &rvs_exchange_caps_dirs_BIS,
        &rvs_rename_no_replace_BIS,
    )
}

fn rvs_lock_caps_migration_BIS(
    caps: &Path,
) -> Result<crate::fs_guard::RivusDirectoryLock, CapsMigrationError> {
    let parent = caps
        .parent()
        .expect("never: caps directory has a project parent");
    match crate::fs_guard::rvs_try_lock_directory_BIS(parent) {
        Ok(lock) => Ok(lock),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Err(CapsMigrationError::MigrationInProgress {
                path: parent.to_path_buf(),
            })
        }
        Err(source) => Err(CapsMigrationError::LockAcquire {
            path: parent.to_path_buf(),
            source,
        }),
    }
}

fn rvs_recover_interrupted_publish_BIS(
    caps: &Path,
    backup: &Path,
) -> Result<bool, CapsMigrationError> {
    let parent = caps
        .parent()
        .expect("never: caps directory has a project parent");
    let markers = rvs_migration_markers_BIS(caps)?;
    match markers.as_slice() {
        [] => Ok(false),
        [marker] => {
            let _ = CapsMap::rvs_load_dir_BIS(caps).map_err(|source| {
                CapsMigrationError::RecoveryActiveInvalid {
                    path: caps.to_path_buf(),
                    source,
                }
            })?;
            let staging_name = crate::fs_guard::rvs_read_regular_file_no_follow_BIS(marker)
                .map_err(|source| CapsMigrationError::ReadLayer {
                    path: marker.clone(),
                    source,
                })?;
            let staging_name = staging_name.trim();
            let staging_path = Path::new(staging_name);
            if staging_name.is_empty()
                || staging_path.components().count() != 1
                || !staging_name.starts_with(".caps.")
                || !crate::fs_guard::rvs_is_atomic_sibling_temp_name(staging_name)
            {
                return Err(CapsMigrationError::InvalidRecoveryMarker {
                    message: format!(
                        "{} contains invalid staging name {staging_name:?}",
                        marker.display()
                    ),
                });
            }
            let staging = parent.join(staging_path);
            let staging_exists = rvs_path_exists_BIS(&staging)?;
            let backup_exists = rvs_path_exists_BIS(backup)?;
            let recovery_source = match (staging_exists, backup_exists) {
                (true, false) => &staging,
                (false, true) => backup,
                (true, true) => {
                    return Err(CapsMigrationError::RecoverySourcesConflict {
                        staging,
                        backup: backup.to_path_buf(),
                    });
                }
                (false, false) => {
                    return Err(CapsMigrationError::RecoverySourceMissing {
                        staging,
                        backup: backup.to_path_buf(),
                    });
                }
            };
            let _ = rvs_require_real_caps_dir_BIS(recovery_source)?;
            let converted = rvs_collect_converted_layers_BIS(recovery_source)?;
            if !rvs_recovery_candidate_matches_active_BIS(caps, &converted)? {
                return Err(CapsMigrationError::RecoveryMismatch {
                    staging: recovery_source.to_path_buf(),
                });
            }
            rvs_sync_legacy_caps_dir_BIS(recovery_source)?;
            if staging_exists {
                rvs_rename_no_replace_BIS(&staging, backup).map_err(|source| {
                    CapsMigrationError::RecoveryPublish {
                        staging: staging.to_path_buf(),
                        backup: backup.to_path_buf(),
                        source,
                    }
                })?;
            }
            rvs_finalize_published_caps_BIS(caps, parent)?;
            Ok(true)
        }
        _ => Err(CapsMigrationError::AmbiguousRecovery {
            path: parent.to_path_buf(),
        }),
    }
}

fn rvs_publish_staged_caps_BIS(
    caps: &Path,
    backup: &Path,
    staging: &Path,
    exchange: &impl Fn(&Path, &Path) -> std::io::Result<()>,
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), CapsMigrationError> {
    if let Err(source) = exchange(caps, staging) {
        return Err(CapsMigrationError::PublishExchange {
            caps: caps.to_path_buf(),
            staging: staging.to_path_buf(),
            source,
            cleanup: rvs_cleanup_staging_BIS(staging),
        });
    }
    if let Err(error) = rvs_validate_exchanged_source_BIS(caps, staging) {
        return Err(CapsMigrationError::PublishValidation {
            staging: staging.to_path_buf(),
            message: error.to_string(),
            rollback: rvs_rollback_published_exchange_BIS(caps, staging, exchange),
        });
    }
    rvs_sync_legacy_caps_dir_BIS(staging)?;
    if let Err(source) = rename(staging, backup) {
        return Err(CapsMigrationError::BackupPublish {
            staging: staging.to_path_buf(),
            backup: backup.to_path_buf(),
            source,
            rollback: rvs_rollback_published_exchange_BIS(caps, staging, exchange),
        });
    }
    let parent = caps
        .parent()
        .expect("never: caps directory has a project parent");
    rvs_finalize_published_caps_BIS(caps, parent)
}

fn rvs_validate_exchanged_source_BIS(
    active: &Path,
    exchanged_source: &Path,
) -> Result<(), CapsMigrationError> {
    let converted = rvs_collect_converted_layers_BIS(exchanged_source)?;
    if !rvs_recovery_candidate_matches_active_BIS(active, &converted)? {
        return Err(CapsMigrationError::SemanticMismatch);
    }
    Ok(())
}

fn rvs_rollback_published_exchange_BIS(
    caps: &Path,
    staging: &Path,
    exchange: &impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> String {
    match exchange(caps, staging) {
        Ok(()) => {
            let sync_error = caps.parent().map(rvs_sync_dir_BIS).transpose().err();
            if let Some(error) = sync_error {
                format!(
                    "original caps exchanged back but the parent directory could not be synced: {error}; staged v2 directory retained at {}",
                    staging.display()
                )
            } else {
                let cleanup = rvs_cleanup_staging_BIS(staging);
                format!("original caps restored{cleanup}")
            }
        }
        Err(error) => format!(
            "failed to restore original caps automatically: {error}; original caps remain at {}",
            staging.display()
        ),
    }
}

fn rvs_finalize_published_caps_BIS(caps: &Path, parent: &Path) -> Result<(), CapsMigrationError> {
    rvs_sync_dir_BIS(parent).map_err(|source| CapsMigrationError::PublishSync {
        path: parent.to_path_buf(),
        source,
    })?;
    rvs_sync_caps_dir_BIS(caps).map_err(|source| CapsMigrationError::PublishSync {
        path: caps.to_path_buf(),
        source,
    })?;
    rvs_sync_dir_BIS(parent).map_err(|source| CapsMigrationError::PublishSync {
        path: parent.to_path_buf(),
        source,
    })?;
    let markers = rvs_remove_migration_markers_BIS(caps)?;
    if let Err(source) = rvs_sync_dir_BIS(caps) {
        let recovery = match rvs_restore_migration_markers_BIS(caps, &markers) {
            Ok(()) => "recovery markers restored".to_string(),
            Err(error) => format!("cannot restore recovery markers: {error}"),
        };
        return Err(CapsMigrationError::MarkerFinalizeSync {
            path: caps.to_path_buf(),
            source,
            recovery,
        });
    }
    Ok(())
}

fn rvs_write_migration_marker_BIS(staging: &Path) -> Result<(), String> {
    let staging_name = staging
        .file_name()
        .expect("never: staging directory has a file name")
        .to_string_lossy();
    let marker = staging.join(format!(".caps-migration.{}.0.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|error| {
            format!(
                "cannot create caps migration marker {}: {error}",
                marker.display()
            )
        })?;
    file.write_all(staging_name.as_bytes()).map_err(|error| {
        format!(
            "cannot write caps migration marker {}: {error}",
            marker.display()
        )
    })
}

fn rvs_migration_markers_BIS(dir: &Path) -> Result<Vec<PathBuf>, CapsMigrationError> {
    let entries = std::fs::read_dir(dir).map_err(|source| CapsMigrationError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut markers = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CapsMigrationError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".caps-migration.")
            && crate::fs_guard::rvs_is_atomic_sibling_temp_name(&name)
        {
            let file_type = entry
                .file_type()
                .map_err(|source| CapsMigrationError::Inspect {
                    path: entry.path(),
                    source,
                })?;
            if !file_type.is_file() {
                return Err(CapsMigrationError::InvalidRecoveryMarker {
                    message: format!("{} is not a regular file", entry.path().display()),
                });
            }
            markers.push(entry.path());
        }
    }
    markers.sort();
    Ok(markers)
}

fn rvs_remove_migration_markers_BIS(
    dir: &Path,
) -> Result<Vec<MigrationMarker>, CapsMigrationError> {
    let markers = rvs_migration_markers_BIS(dir)?
        .into_iter()
        .map(|path| {
            let content =
                crate::fs_guard::rvs_read_regular_file_no_follow_BIS(&path).map_err(|source| {
                    CapsMigrationError::ReadLayer {
                        path: path.clone(),
                        source,
                    }
                })?;
            Ok(MigrationMarker { path, content })
        })
        .collect::<Result<Vec<_>, CapsMigrationError>>()?;
    for marker in &markers {
        std::fs::remove_file(&marker.path).map_err(|source| CapsMigrationError::MarkerRemove {
            path: marker.path.clone(),
            source,
        })?;
    }
    Ok(markers)
}

fn rvs_restore_migration_markers_BIS(
    dir: &Path,
    markers: &[MigrationMarker],
) -> Result<(), std::io::Error> {
    for marker in markers {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker.path)?;
        file.write_all(marker.content.as_bytes())?;
        file.sync_all()?;
    }
    rvs_sync_dir_BIS(dir)
}

fn rvs_recovery_candidate_matches_active_BIS(
    caps: &Path,
    converted: &[ConvertedLayer],
) -> Result<bool, CapsMigrationError> {
    let mut active_layers = BTreeMap::new();
    let entries = std::fs::read_dir(caps).map_err(|source| CapsMigrationError::ReadDir {
        path: caps.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CapsMigrationError::ReadDir {
            path: caps.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        if crate::fs_guard::rvs_is_atomic_sibling_temp_name(&name.to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| CapsMigrationError::Inspect {
                path: path.clone(),
                source,
            })?;
        if !file_type.is_file() {
            return Err(CapsMigrationError::LayerNotFile { path });
        }
        let content =
            crate::fs_guard::rvs_read_regular_file_no_follow_BIS(&path).map_err(|source| {
                CapsMigrationError::ReadLayer {
                    path: path.clone(),
                    source,
                }
            })?;
        let map = CapsMap::rvs_parse(&content)
            .map_err(|source| CapsMigrationError::Validate { source })?;
        active_layers.insert(name, rvs_semantic_knowledge(&map));
    }
    let converted_layers: BTreeMap<OsString, _> = converted
        .iter()
        .map(|layer| (layer.name.clone(), rvs_semantic_knowledge(&layer.map)))
        .collect();
    Ok(active_layers == converted_layers)
}

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
fn rvs_exchange_caps_dirs_BIS(left: &Path, right: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        left,
        rustix::fs::CWD,
        right,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .map_err(std::io::Error::from)
}

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
fn rvs_rename_no_replace_BIS(from: &Path, to: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_vendor = "apple")))]
fn rvs_exchange_caps_dirs_BIS(_left: &Path, _right: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic caps directory exchange is unsupported on this platform",
    ))
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_vendor = "apple")))]
fn rvs_rename_no_replace_BIS(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no-replace caps backup rename is unsupported on this platform",
    ))
}

fn rvs_sync_caps_dir_BIS(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            crate::fs_guard::rvs_sync_regular_file_no_follow_BIS(&entry.path())?;
        }
    }
    rvs_sync_dir_BIS(dir)
}

fn rvs_sync_legacy_caps_dir_BIS(dir: &Path) -> Result<(), CapsMigrationError> {
    rvs_sync_caps_dir_BIS(dir).map_err(|source| CapsMigrationError::LegacySync {
        path: dir.to_path_buf(),
        source,
    })
}

#[cfg(test)]
static RVS_SYNC_DIR_FAULT: std::sync::Mutex<Option<(PathBuf, usize)>> = std::sync::Mutex::new(None);

fn rvs_sync_dir_BIS(dir: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    let injected_failure = {
        let mut fault = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned");
        if let Some((target, remaining_calls)) = fault.as_mut() {
            if target == dir {
                debug_assert!(
                    *remaining_calls > 0,
                    "sync fault call count must be positive"
                );
                *remaining_calls -= 1;
                if *remaining_calls == 0 {
                    let _ = fault.take();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };
    #[cfg(test)]
    if injected_failure {
        return Err(std::io::Error::other("injected directory sync failure"));
    }
    std::fs::File::open(dir)?.sync_all()
}

fn rvs_require_real_caps_dir_BIS(caps: &Path) -> Result<std::fs::Permissions, CapsMigrationError> {
    let metadata =
        std::fs::symlink_metadata(caps).map_err(|source| CapsMigrationError::Inspect {
            path: caps.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() {
        return Err(CapsMigrationError::CapsSymlink {
            path: caps.to_path_buf(),
        });
    }
    if !metadata.is_dir() {
        return Err(CapsMigrationError::CapsNotDirectory {
            path: caps.to_path_buf(),
        });
    }
    Ok(metadata.permissions())
}

fn rvs_path_exists_BIS(path: &Path) -> Result<bool, CapsMigrationError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(CapsMigrationError::Inspect {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn rvs_collect_converted_layers_BIS(
    caps: &Path,
) -> Result<Vec<ConvertedLayer>, CapsMigrationError> {
    let entries = std::fs::read_dir(caps).map_err(|source| CapsMigrationError::ReadDir {
        path: caps.to_path_buf(),
        source,
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CapsMigrationError::ReadDir {
            path: caps.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if crate::fs_guard::rvs_is_atomic_sibling_temp_name(&name) {
            return Err(CapsMigrationError::TemporarySourceEntry { path });
        }
        let file_type = entry
            .file_type()
            .map_err(|source| CapsMigrationError::Inspect {
                path: path.clone(),
                source,
            })?;
        if !file_type.is_file() {
            return Err(CapsMigrationError::LayerNotFile { path });
        }
        paths.push(path);
    }
    rvs_sort_by_layer_M(&mut paths);

    paths
        .into_iter()
        .map(|path| {
            let content =
                crate::fs_guard::rvs_read_regular_file_no_follow_BIS(&path).map_err(|source| {
                    CapsMigrationError::ReadLayer {
                        path: path.clone(),
                        source,
                    }
                })?;
            let permissions = std::fs::symlink_metadata(&path)
                .map_err(|source| CapsMigrationError::Inspect {
                    path: path.clone(),
                    source,
                })?
                .permissions();
            let name = path
                .file_name()
                .expect("never: caps layer path has a file name")
                .to_os_string();
            let map =
                rvs_convert_v1_layer(&name.to_string_lossy(), &content).map_err(|source| {
                    CapsMigrationError::ParseLayer {
                        path: path.clone(),
                        source,
                    }
                })?;
            let rendered = map.rvs_render_v2();
            let _ = CapsMap::rvs_parse(&rendered)
                .map_err(|source| CapsMigrationError::Validate { source })?;
            Ok(ConvertedLayer {
                name,
                map,
                rendered,
                permissions,
            })
        })
        .collect()
}

fn rvs_convert_v1_layer(layer: &str, content: &str) -> Result<CapsMap, LegacyCapsError> {
    if let Some(line) = content
        .lines()
        .position(|line| line.trim() == crate::capsmap::CAPS_V2_HEADER)
    {
        return Err(LegacyCapsError::AlreadyV2 { line: line + 1 });
    }
    let parsed = rvs_parse_v1(content)?;
    let manually_maintained_layer = matches!(layer, "seed" | "suppress" | "ext");
    let mut map = CapsMap::rvs_new();
    map.rvs_extend_info_entries_M(parsed.into_iter().map(|(key, caps)| {
        let info = if manually_maintained_layer {
            CapabilityInfo::rvs_explicit(caps)
        } else {
            CapabilityInfo::rvs_migrated_v1(caps, CapabilityCompleteness::Unknown)
        };
        (key, info)
    }));
    Ok(map)
}

fn rvs_validate_staged_layers_BIS(
    staging: &Path,
    converted: &[ConvertedLayer],
) -> Result<(), CapsMigrationError> {
    for layer in converted {
        let path = staging.join(&layer.name);
        let content =
            crate::fs_guard::rvs_read_regular_file_no_follow_BIS(&path).map_err(|source| {
                CapsMigrationError::ReadLayer {
                    path: path.clone(),
                    source,
                }
            })?;
        let staged = CapsMap::rvs_parse(&content)
            .map_err(|source| CapsMigrationError::Validate { source })?;
        if rvs_semantic_knowledge(&staged) != rvs_semantic_knowledge(&layer.map) {
            return Err(CapsMigrationError::LayerSemanticMismatch {
                layer: layer.name.to_string_lossy().into_owned(),
            });
        }
    }
    Ok(())
}

fn rvs_parse_v1(content: &str) -> Result<BTreeMap<CapsMapKey, CapabilitySet>, LegacyCapsError> {
    let mut entries = BTreeMap::new();
    let mut first_lines = BTreeMap::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = trimmed
            .split_once('=')
            .ok_or(LegacyCapsError::MissingSeparator { line })?;
        if key.trim().is_empty() {
            return Err(LegacyCapsError::EmptyKey { line });
        }
        let key = CapsMapKey::rvs_new(key.trim().to_string());
        if let Some(first_line) = first_lines.get(&key) {
            return Err(LegacyCapsError::DuplicateKey {
                key,
                first_line: *first_line,
                line,
            });
        }
        let value = value.split('#').next().unwrap_or("").trim();
        let caps =
            CapabilitySet::rvs_from_str(value).map_err(|source| LegacyCapsError::InvalidCaps {
                key: key.clone(),
                caps: value.to_string(),
                line,
                source,
            })?;
        first_lines.insert(key.clone(), line);
        entries.insert(key, caps);
    }
    Ok(entries)
}

fn rvs_create_staging_dir_BIS(caps: &Path) -> Result<PathBuf, CapsMigrationError> {
    for attempt in 0..100usize {
        debug_assert!(attempt < 100, "caps migration staging retry bound");
        let candidate = crate::fs_guard::rvs_atomic_sibling_temp_path_S(caps, attempt);
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(CapsMigrationError::StageCreate {
                    path: candidate,
                    source,
                });
            }
        }
    }
    Err(CapsMigrationError::StageCreateExhausted)
}

fn rvs_stage_operation_error_BIS(staging: &Path, message: String) -> CapsMigrationError {
    CapsMigrationError::StageOperation {
        message,
        cleanup: rvs_cleanup_staging_BIS(staging),
    }
}

fn rvs_cleanup_staging_BIS(staging: &Path) -> String {
    match crate::workspace::rvs_clean_dir_BIS(staging) {
        Ok(()) => String::new(),
        Err(error) => format!("; additionally cannot remove staging directory: {error}"),
    }
}

fn rvs_semantic_caps(map: &CapsMap) -> BTreeMap<String, String> {
    map.rvs_iter()
        .map(|(key, info)| (key.rvs_as_str().to_string(), info.rvs_caps().rvs_letters()))
        .collect()
}

fn rvs_semantic_knowledge(
    map: &CapsMap,
) -> BTreeMap<
    String,
    (
        String,
        crate::capability::CapabilityBasis,
        CapabilityCompleteness,
    ),
> {
    map.rvs_iter()
        .map(|(key, info)| {
            (
                key.rvs_as_str().to_string(),
                (
                    info.rvs_caps().rvs_letters(),
                    info.rvs_basis().clone(),
                    info.rvs_completeness(),
                ),
            )
        })
        .collect()
}

fn rvs_semantic_layers(layers: &[ConvertedLayer]) -> BTreeMap<OsString, BTreeMap<String, String>> {
    layers
        .iter()
        .map(|layer| (layer.name.clone(), rvs_semantic_caps(&layer.map)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::capability::{CapabilityBasis, CapabilityCompleteness};
    use crate::test_support::{
        rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };

    static RVS_SYNC_FAULT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn test_20260716_migration_marker_create_new_rejects_symlink() {
        let project = rvs_make_temp_dir_BIS("caps-migration-marker-symlink");
        let staging = project.join("staging");
        let victim = project.join("victim");
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(&victim, "safe").unwrap();
        let marker = staging.join(format!(".caps-migration.{}.0.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, &marker).unwrap();

        let result = rvs_write_migration_marker_BIS(&staging);
        let victim_content = std::fs::read_to_string(&victim).unwrap();
        let marker_is_symlink = std::fs::symlink_metadata(&marker)
            .unwrap()
            .file_type()
            .is_symlink();
        let output = format!(
            "result_is_err={}\nvictim={victim_content:?}\nmarker_is_symlink={marker_is_symlink}\n",
            result.is_err(),
        );
        rvs_snapshot_BIS(
            "test_20260716_migration_marker_create_new_rejects_symlink",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(victim_content, "safe");
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_v1_conversion_assigns_layer_knowledge() {
        let std_map = rvs_convert_v1_layer("std", "pure=\neffect=BI\n").unwrap();
        let ext_map = rvs_convert_v1_layer("ext", "effect=S\n").unwrap();
        let generated_map = rvs_convert_v1_layer("custom-generated", "effect=I\n").unwrap();
        let std_info = std_map.rvs_lookup_info("effect").unwrap();
        let ext_info = ext_map.rvs_lookup_info("effect").unwrap();
        let generated_info = generated_map.rvs_lookup_info("effect").unwrap();
        let output = format!(
            "std_basis={:?}\nstd_complete={:?}\next_basis={:?}\next_complete={:?}\ngenerated_basis={:?}\ngenerated_complete={:?}\nrendered={}\n",
            std_info.rvs_basis(),
            std_info.rvs_completeness(),
            ext_info.rvs_basis(),
            ext_info.rvs_completeness(),
            generated_info.rvs_basis(),
            generated_info.rvs_completeness(),
            std_map.rvs_render_v2(),
        );
        rvs_snapshot_BIS(
            "test_20260715_v1_conversion_assigns_layer_knowledge",
            &output,
        );

        assert_eq!(std_info.rvs_basis(), &CapabilityBasis::MigratedV1);
        assert_eq!(std_info.rvs_completeness(), CapabilityCompleteness::Unknown);
        assert_eq!(ext_info.rvs_basis(), &CapabilityBasis::Explicit);
        assert_eq!(
            ext_info.rvs_completeness(),
            CapabilityCompleteness::Complete
        );
        assert_eq!(generated_info.rvs_basis(), &CapabilityBasis::MigratedV1);
        assert_eq!(
            generated_info.rvs_completeness(),
            CapabilityCompleteness::Unknown
        );
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_caps_directory_migration_preserves_effective_caps() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-success");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(caps.join("std"), "winner=B\nstd_only=I\n").unwrap();
        std::fs::write(caps.join("ext"), "winner=S\n").unwrap();

        rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap();

        let migrated = CapsMap::rvs_load_dir_BIS(&caps).unwrap();
        let winner = migrated.rvs_lookup("winner").unwrap().rvs_letters();
        let std_only = migrated.rvs_lookup("std_only").unwrap().rvs_letters();
        let backup_std = std::fs::read_to_string(backup.join("std")).unwrap();
        let new_std = std::fs::read_to_string(caps.join("std")).unwrap();
        let output = format!(
            "winner={winner}\nstd_only={std_only}\nbackup_std={backup_std:?}\nnew_header={:?}\n",
            new_std.lines().next(),
        );
        rvs_snapshot_BIS(
            "test_20260715_caps_directory_migration_preserves_effective_caps",
            &output,
        );

        assert_eq!(winner, "S");
        assert_eq!(std_only, "I");
        assert_eq!(backup_std, "winner=B\nstd_only=I\n");
        assert_eq!(new_std.lines().next(), Some(crate::capsmap::CAPS_V2_HEADER));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(all(
        unix,
        any(target_os = "android", target_os = "linux", target_vendor = "apple")
    ))]
    #[test]
    fn test_20260716_caps_migration_preserves_unix_modes_without_following_links() {
        use std::os::unix::fs::PermissionsExt as _;

        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-unix-modes");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(caps.join("seed"), "seed=S\n").unwrap();
        std::fs::write(caps.join("ext"), "ext=I\n").unwrap();
        std::fs::set_permissions(&caps, std::fs::Permissions::from_mode(0o710)).unwrap();
        std::fs::set_permissions(caps.join("seed"), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        std::fs::set_permissions(caps.join("ext"), std::fs::Permissions::from_mode(0o640)).unwrap();

        rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap();

        let rvs_mode = |path: &Path| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert!(!metadata.file_type().is_symlink());
            metadata.permissions().mode() & 0o777
        };
        let output = format!(
            "caps_mode={:o}\nseed_mode={:o}\next_mode={:o}\nbackup_caps_mode={:o}\nbackup_seed_mode={:o}\nbackup_ext_mode={:o}\n",
            rvs_mode(&caps),
            rvs_mode(&caps.join("seed")),
            rvs_mode(&caps.join("ext")),
            rvs_mode(&backup),
            rvs_mode(&backup.join("seed")),
            rvs_mode(&backup.join("ext")),
        );
        rvs_snapshot_BIS(
            "test_20260716_caps_migration_preserves_unix_modes_without_following_links",
            &output,
        );

        assert_eq!(rvs_mode(&caps), 0o710);
        assert_eq!(rvs_mode(&caps.join("seed")), 0o600);
        assert_eq!(rvs_mode(&caps.join("ext")), 0o640);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260716_caps_migration_rejects_already_v2_layer() {
        let error = rvs_convert_v1_layer("seed", &format!("{}\n", crate::capsmap::CAPS_V2_HEADER))
            .unwrap_err();
        let output = format!("{error}\n");
        rvs_snapshot_BIS(
            "test_20260716_caps_migration_rejects_already_v2_layer",
            &output,
        );

        assert!(output.contains("v2"));
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_migrate_caps_command_handler_converts_project() {
        let project = rvs_make_cargo_project_BIS(
            "caps-v1-to-v2-handler",
            "caps-migration-handler",
            &[("src/lib.rs", "pub fn rvs_value() -> u32 { 1 }\n")],
        );
        std::fs::create_dir(project.join("caps")).unwrap();
        std::fs::write(project.join("caps/seed"), "demo::value=S\n").unwrap();

        let result = rvs_run_migrate_caps_BIS(&project);
        let migrated = CapsMap::rvs_load_dir_BIS(&project.join("caps")).unwrap();
        let output = format!(
            "result={result:?}\nvalue={}\nbackup={}\n",
            migrated.rvs_lookup("demo::value").unwrap().rvs_letters(),
            project.join("caps.v1-backup").is_dir(),
        );
        rvs_snapshot_BIS(
            "test_20260715_migrate_caps_command_handler_converts_project",
            &output,
        );

        assert!(result.is_ok());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_migrate_caps_cli_dispatches_and_reports_exit_status() {
        let project = rvs_make_cargo_project_BIS(
            "caps-v1-to-v2-cli",
            "caps-migration-cli",
            &[("src/lib.rs", "pub fn rvs_value() -> u32 { 1 }\n")],
        );
        std::fs::create_dir(project.join("caps")).unwrap();
        std::fs::write(project.join("caps/seed"), "demo::value=S\n").unwrap();
        let executable = crate::workspace::rvs_current_wrapper_exe_BIS().unwrap();
        let binary_dir = executable
            .parent()
            .expect("never: test executable has a parent directory");
        let mut search_path = vec![binary_dir.to_path_buf()];
        search_path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let search_path = std::env::join_paths(search_path).unwrap();

        let success = std::process::Command::new("cargo")
            .arg("rivus")
            .arg("migrate-caps")
            .arg(&project)
            .env("PATH", search_path)
            .output()
            .unwrap();
        let repeated = std::process::Command::new(executable)
            .arg("migrate-caps")
            .arg(&project)
            .output()
            .unwrap();
        let success_stdout = String::from_utf8_lossy(&success.stdout);
        let repeated_stderr = String::from_utf8_lossy(&repeated.stderr);
        let output = format!(
            "success={}\nmessage={}\nrepeated_success={}\nbackup_error={}\n",
            success.status.success(),
            success_stdout.contains("Migrated"),
            repeated.status.success(),
            repeated_stderr.contains("caps v1 backup already exists"),
        );
        rvs_snapshot_BIS(
            "test_20260715_migrate_caps_cli_dispatches_and_reports_exit_status",
            &output,
        );

        assert!(success.status.success(), "{success_stdout}");
        assert!(!repeated.status.success());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_concurrent_caps_migration_is_rejected() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-concurrent-lock");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        let lock = rvs_lock_caps_migration_BIS(&caps).unwrap();

        let error = rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap_err();
        let output = format!("{error}\n").replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_concurrent_caps_migration_is_rejected",
            &output,
        );

        assert!(matches!(
            error,
            CapsMigrationError::MigrationInProgress { .. }
        ));
        drop(lock);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_interrupted_publish_is_recovered_on_retry() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-interrupted-recovery");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        rvs_exchange_caps_dirs_BIS(&caps, &staging).unwrap();

        rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap();
        let active = CapsMap::rvs_load_dir_BIS(&caps).unwrap();
        let output = format!(
            "active={}\nbackup={}\nstaging_exists={}\n",
            active.rvs_lookup("value").unwrap().rvs_letters(),
            std::fs::read_to_string(backup.join("seed")).unwrap().trim(),
            staging.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260715_interrupted_publish_is_recovered_on_retry",
            &output,
        );

        assert!(!staging.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_interrupted_publish_after_backup_rename_is_recovered_on_retry() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-interrupted-after-backup");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        rvs_exchange_caps_dirs_BIS(&caps, &staging).unwrap();
        rvs_rename_no_replace_BIS(&staging, &backup).unwrap();

        rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap();
        let active = CapsMap::rvs_load_dir_BIS(&caps).unwrap();
        let marker_count = rvs_migration_markers_BIS(&caps).unwrap().len();
        let output = format!(
            "active={}\nbackup={}\nmarker_count={marker_count}\n",
            active.rvs_lookup("value").unwrap().rvs_letters(),
            std::fs::read_to_string(backup.join("seed")).unwrap().trim(),
        );
        rvs_snapshot_BIS(
            "test_20260715_interrupted_publish_after_backup_rename_is_recovered_on_retry",
            &output,
        );

        assert_eq!(marker_count, 0);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_interrupted_publish_rejects_mismatched_staging() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-recovery-mismatch");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(
            caps.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        std::fs::write(staging.join("seed"), "value=I\n").unwrap();
        let marker = caps.join(format!(".caps-migration.{}.0.tmp", std::process::id()));
        std::fs::write(
            marker,
            staging
                .file_name()
                .expect("never: staging path has a file name")
                .as_encoded_bytes(),
        )
        .unwrap();

        let error = rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap_err();
        let output = format!(
            "error={error}\nactive={}\nbackup_exists={}\nstaging_exists={}\n",
            CapsMap::rvs_load_dir_BIS(&caps)
                .unwrap()
                .rvs_lookup("value")
                .unwrap()
                .rvs_letters(),
            backup.exists(),
            staging.exists(),
        )
        .replace(&staging.to_string_lossy().into_owned(), "$STAGING")
        .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_interrupted_publish_rejects_mismatched_staging",
            &output,
        );

        assert!(matches!(error, CapsMigrationError::RecoveryMismatch { .. }));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_interrupted_publish_rejects_invalid_active_v2() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-invalid-active-recovery");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "not-v2\n").unwrap();
        std::fs::write(staging.join("seed"), "value=S\n").unwrap();
        let marker = caps.join(format!(".caps-migration.{}.0.tmp", std::process::id()));
        std::fs::write(
            marker,
            staging
                .file_name()
                .expect("never: staging path has a file name")
                .as_encoded_bytes(),
        )
        .unwrap();

        let error = rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap_err();
        let output = format!(
            "kind={}\nactive={}\nbackup_exists={}\nstaging_exists={}\n",
            matches!(error, CapsMigrationError::RecoveryActiveInvalid { .. }),
            std::fs::read_to_string(caps.join("seed")).unwrap().trim(),
            backup.exists(),
            staging.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260715_interrupted_publish_rejects_invalid_active_v2",
            &output,
        );

        assert!(matches!(
            error,
            CapsMigrationError::RecoveryActiveInvalid { .. }
        ));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_migration_rejects_temporary_shaped_source_entry() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-temporary-source");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(caps.join(".custom.1.0.tmp"), "hidden=I\n").unwrap();

        let error = rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap_err();
        let output = format!(
            "kind={}\nsource_exists={}\nbackup_exists={}\n",
            matches!(error, CapsMigrationError::TemporarySourceEntry { .. }),
            caps.join(".custom.1.0.tmp").exists(),
            backup.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260715_migration_rejects_temporary_shaped_source_entry",
            &output,
        );

        assert!(matches!(
            error,
            CapsMigrationError::TemporarySourceEntry { .. }
        ));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_staged_layer_validation_detects_semantic_change() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-layer-validation");
        let staging = project.join("staging");
        std::fs::create_dir(&staging).unwrap();
        let map = rvs_convert_v1_layer("seed", "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=I\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        let converted = vec![ConvertedLayer {
            name: OsString::from("seed"),
            rendered: map.rvs_render_v2(),
            map,
            permissions: std::fs::metadata(staging.join("seed"))
                .unwrap()
                .permissions(),
        }];

        let error = rvs_validate_staged_layers_BIS(&staging, &converted).unwrap_err();
        let output = format!("{error}\n");
        rvs_snapshot_BIS(
            "test_20260715_staged_layer_validation_detects_semantic_change",
            &output,
        );

        assert!(matches!(
            error,
            CapsMigrationError::LayerSemanticMismatch { .. }
        ));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_staged_layer_validation_detects_knowledge_change() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-knowledge-validation");
        let staging = project.join("staging");
        std::fs::create_dir(&staging).unwrap();
        let map = rvs_convert_v1_layer("custom-generated", "value=S\n").unwrap();
        std::fs::write(
            staging.join("custom-generated"),
            rvs_convert_v1_layer("ext", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        let converted = vec![ConvertedLayer {
            name: OsString::from("custom-generated"),
            rendered: map.rvs_render_v2(),
            map,
            permissions: std::fs::metadata(staging.join("custom-generated"))
                .unwrap()
                .permissions(),
        }];

        let error = rvs_validate_staged_layers_BIS(&staging, &converted).unwrap_err();
        let output = format!("{error}\n");
        rvs_snapshot_BIS(
            "test_20260715_staged_layer_validation_detects_knowledge_change",
            &output,
        );

        assert!(matches!(
            error,
            CapsMigrationError::LayerSemanticMismatch { .. }
        ));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_invalid_v1_migration_preserves_original_directory() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-invalid");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(caps.join("seed"), "broken=Z\n").unwrap();

        let result = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let original = std::fs::read_to_string(caps.join("seed")).unwrap();
        let output = format!(
            "error={}\noriginal={original:?}\nbackup_exists={}\n",
            result.unwrap_err(),
            backup.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260715_invalid_v1_migration_preserves_original_directory",
            &output.replace(&project.to_string_lossy().into_owned(), "$TMP"),
        );

        assert_eq!(original, "broken=Z\n");
        assert!(!backup.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_existing_backup_blocks_caps_migration() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-existing-backup");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&backup).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();

        let result = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let output = format!("{}\n", result.unwrap_err())
            .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_existing_backup_blocks_caps_migration",
            &output,
        );

        assert_eq!(
            std::fs::read_to_string(caps.join("seed")).unwrap(),
            "value=S\n"
        );
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_publish_failure_restores_original_caps() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-publish-rollback");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = project.join("staging");
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        let exchange_calls = Cell::new(0usize);
        let exchange = |left: &Path, right: &Path| {
            exchange_calls.set(exchange_calls.get() + 1);
            rvs_exchange_caps_dirs_BIS(left, right)
        };
        let rename_calls = Cell::new(0usize);
        let rename = |from: &Path, to: &Path| {
            let _ = (from, to);
            rename_calls.set(rename_calls.get() + 1);
            Err(std::io::Error::other("backup publish failed"))
        };

        let result = rvs_publish_staged_caps_BIS(&caps, &backup, &staging, &exchange, &rename);
        let original = std::fs::read_to_string(caps.join("seed")).unwrap();
        let output = format!(
            "error={}\nexchange_calls={}\nrename_calls={}\noriginal={}\nbackup_exists={}\nstaging_exists={}\n",
            result.unwrap_err(),
            exchange_calls.get(),
            rename_calls.get(),
            if original.starts_with(crate::capsmap::CAPS_V2_HEADER) {
                "new"
            } else {
                "old"
            },
            backup.exists(),
            staging.exists(),
        )
        .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_publish_failure_restores_original_caps",
            &output,
        );

        assert_eq!(exchange_calls.get(), 2);
        assert_eq!(rename_calls.get(), 1);
        assert!(!backup.exists());
        assert!(!staging.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_rollback_failure_preserves_original_in_staging() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-rollback-failure");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = project.join("staging");
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        let exchange_calls = Cell::new(0usize);
        let exchange = |left: &Path, right: &Path| {
            exchange_calls.set(exchange_calls.get() + 1);
            if exchange_calls.get() == 1 {
                rvs_exchange_caps_dirs_BIS(left, right)
            } else {
                Err(std::io::Error::other("rollback exchange failed"))
            }
        };

        let error =
            rvs_publish_staged_caps_BIS(&caps, &backup, &staging, &exchange, &|_from, _to| {
                Err(std::io::Error::other("backup publish failed"))
            })
            .unwrap_err();
        let published = std::fs::read_to_string(caps.join("seed")).unwrap();
        let original_in_staging = std::fs::read_to_string(staging.join("seed")).unwrap();
        let output = format!(
            "error={error}\nexchange_calls={}\npublished={}\noriginal_in_staging={}\nbackup_exists={}\n",
            exchange_calls.get(),
            if published.starts_with(crate::capsmap::CAPS_V2_HEADER) {
                "new"
            } else {
                "old"
            },
            if original_in_staging.starts_with(crate::capsmap::CAPS_V2_HEADER) {
                "new"
            } else {
                "old"
            },
            backup.exists(),
        )
        .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_rollback_failure_preserves_original_in_staging",
            &output,
        );

        assert_eq!(exchange_calls.get(), 2);
        assert!(!original_in_staging.starts_with(crate::capsmap::CAPS_V2_HEADER));
        assert!(!backup.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn test_20260715_exchange_failure_cleans_staging_without_touching_caps() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-exchange-failure");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = project.join("staging");
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "old").unwrap();
        std::fs::write(staging.join("seed"), "new").unwrap();

        let result = rvs_publish_staged_caps_BIS(
            &caps,
            &backup,
            &staging,
            &|_left, _right| Err(std::io::Error::other("exchange failed")),
            &|_from, _to| Ok(()),
        );
        let output = format!(
            "error={}\noriginal={}\nbackup_exists={}\nstaging_exists={}\n",
            result.unwrap_err(),
            std::fs::read_to_string(caps.join("seed")).unwrap(),
            backup.exists(),
            staging.exists(),
        )
        .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_exchange_failure_cleans_staging_without_touching_caps",
            &output,
        );

        assert!(!backup.exists());
        assert!(!staging.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_post_exchange_source_change_rolls_back_stale_v2() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-post-exchange-change");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = project.join("staging");
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        let exchange_calls = Cell::new(0usize);
        let exchange = |left: &Path, right: &Path| {
            exchange_calls.set(exchange_calls.get() + 1);
            if exchange_calls.get() == 1 {
                std::fs::write(left.join("seed"), "value=I\n").unwrap();
            }
            rvs_exchange_caps_dirs_BIS(left, right)
        };
        let rename_calls = Cell::new(0usize);
        let rename = |from: &Path, to: &Path| {
            rename_calls.set(rename_calls.get() + 1);
            rvs_rename_no_replace_BIS(from, to)
        };

        let result = rvs_publish_staged_caps_BIS(&caps, &backup, &staging, &exchange, &rename);
        let active = std::fs::read_to_string(caps.join("seed")).unwrap();
        let output = format!(
            "result_is_error={}\nexchange_calls={}\nrename_calls={}\nactive_matches_latest_source={}\nbackup_exists={}\nstaging_exists={}\n",
            result.is_err(),
            exchange_calls.get(),
            rename_calls.get(),
            active == "value=I\n",
            backup.exists(),
            staging.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260716_post_exchange_source_change_rolls_back_stale_v2",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(exchange_calls.get(), 2);
        assert_eq!(rename_calls.get(), 0);
        assert_eq!(active, "value=I\n");
        assert!(!backup.exists());
        assert!(!staging.exists());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_publish_sync_failure_preserves_recovery_marker() {
        let _fault_guard = RVS_SYNC_FAULT_TEST_LOCK
            .lock()
            .expect("never: sync fault test mutex should not be poisoned");
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-sync-recovery-marker");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        *RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned") =
            Some((project.clone(), 2));

        let first = rvs_publish_staged_caps_BIS(
            &caps,
            &backup,
            &staging,
            &rvs_exchange_caps_dirs_BIS,
            &rvs_rename_no_replace_BIS,
        );
        let _ = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned")
            .take();
        let marker_after_failure = rvs_migration_markers_BIS(&caps).unwrap().len();
        let backup_after_failure = backup.exists();
        let retry = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let marker_after_retry = rvs_migration_markers_BIS(&caps).unwrap().len();
        let active = CapsMap::rvs_load_dir_BIS(&caps).unwrap();
        let output = format!(
            "first_error={}\nmarker_after_failure={marker_after_failure}\nbackup_after_failure={backup_after_failure}\nretry_ok={}\nmarker_after_retry={marker_after_retry}\nactive={}\n",
            first.is_err(),
            retry.is_ok(),
            active.rvs_lookup("value").unwrap().rvs_letters(),
        );
        rvs_snapshot_BIS(
            "test_20260716_publish_sync_failure_preserves_recovery_marker",
            &output,
        );

        assert!(first.is_err());
        assert_eq!(marker_after_failure, 1);
        assert!(backup_after_failure);
        assert!(retry.is_ok());
        assert_eq!(marker_after_retry, 0);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_marker_removal_sync_failure_remains_recoverable() {
        let _fault_guard = RVS_SYNC_FAULT_TEST_LOCK
            .lock()
            .expect("never: sync fault test mutex should not be poisoned");
        let project = rvs_make_temp_dir_BIS("caps-v1-marker-removal-sync-recovery");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        *RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned") =
            Some((caps.clone(), 2));

        let publish = rvs_publish_staged_caps_BIS(
            &caps,
            &backup,
            &staging,
            &rvs_exchange_caps_dirs_BIS,
            &rvs_rename_no_replace_BIS,
        );
        let _ = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned")
            .take();
        let marker_after_failure = rvs_migration_markers_BIS(&caps).unwrap().len();
        let backup_after_failure = backup.exists();
        let retry = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let marker_after_retry = rvs_migration_markers_BIS(&caps).unwrap().len();
        let active = CapsMap::rvs_load_dir_BIS(&caps)
            .ok()
            .and_then(|map| map.rvs_lookup("value").map(CapabilitySet::rvs_letters))
            .unwrap_or_else(|| "<unavailable>".to_string());
        let output = format!(
            "publish_error={}\nmarker_after_failure={marker_after_failure}\nbackup_after_failure={backup_after_failure}\nretry_ok={}\nmarker_after_retry={marker_after_retry}\nactive={active}\n",
            publish.is_err(),
            retry.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_marker_removal_sync_failure_remains_recoverable",
            &output,
        );

        assert!(publish.is_err());
        assert_eq!(marker_after_failure, 1);
        assert!(backup_after_failure);
        assert!(retry.is_ok());
        assert_eq!(marker_after_retry, 0);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_legacy_tree_sync_failure_remains_recoverable() {
        let _fault_guard = RVS_SYNC_FAULT_TEST_LOCK
            .lock()
            .expect("never: sync fault test mutex should not be poisoned");
        let project = rvs_make_temp_dir_BIS("caps-v1-legacy-sync-recovery");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();

        *RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned") =
            Some((staging.clone(), 1));
        let publish = rvs_publish_staged_caps_BIS(
            &caps,
            &backup,
            &staging,
            &rvs_exchange_caps_dirs_BIS,
            &rvs_rename_no_replace_BIS,
        );
        let _ = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned")
            .take();
        let marker_after_publish_failure = rvs_migration_markers_BIS(&caps).unwrap().len();
        let staging_after_publish_failure = staging.exists();
        let backup_after_publish_failure = backup.exists();

        *RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned") =
            Some((staging.clone(), 1));
        let recovery = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let _ = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned")
            .take();
        let marker_after_recovery_failure = rvs_migration_markers_BIS(&caps).unwrap().len();
        let staging_after_recovery_failure = staging.exists();
        let backup_after_recovery_failure = backup.exists();
        let retry = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let marker_after_retry = rvs_migration_markers_BIS(&caps).unwrap().len();
        let active = CapsMap::rvs_load_dir_BIS(&caps)
            .ok()
            .and_then(|map| map.rvs_lookup("value").map(CapabilitySet::rvs_letters))
            .unwrap_or_else(|| "<unavailable>".to_string());
        let output = format!(
            "publish_error={}\nmarker_after_publish_failure={marker_after_publish_failure}\nstaging_after_publish_failure={staging_after_publish_failure}\nbackup_after_publish_failure={backup_after_publish_failure}\nrecovery_error={}\nmarker_after_recovery_failure={marker_after_recovery_failure}\nstaging_after_recovery_failure={staging_after_recovery_failure}\nbackup_after_recovery_failure={backup_after_recovery_failure}\nretry_ok={}\nmarker_after_retry={marker_after_retry}\nbackup_after_retry={}\nactive={active}\n",
            publish.is_err(),
            recovery.is_err(),
            retry.is_ok(),
            backup.exists(),
        );
        rvs_snapshot_BIS(
            "test_20260716_legacy_tree_sync_failure_remains_recoverable",
            &output,
        );

        assert!(publish.is_err());
        assert_eq!(marker_after_publish_failure, 1);
        assert!(staging_after_publish_failure);
        assert!(!backup_after_publish_failure);
        assert!(recovery.is_err());
        assert_eq!(marker_after_recovery_failure, 1);
        assert!(staging_after_recovery_failure);
        assert!(!backup_after_recovery_failure);
        assert!(retry.is_ok());
        assert_eq!(marker_after_retry, 0);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_backup_sync_failure_keeps_recovery_marker() {
        let _fault_guard = RVS_SYNC_FAULT_TEST_LOCK
            .lock()
            .expect("never: sync fault test mutex should not be poisoned");
        let project = rvs_make_temp_dir_BIS("caps-v1-backup-sync-recovery");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let staging = crate::fs_guard::rvs_atomic_sibling_temp_path_S(&caps, 0);
        std::fs::create_dir(&caps).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(caps.join("seed"), "value=S\n").unwrap();
        std::fs::write(
            staging.join("seed"),
            rvs_convert_v1_layer("seed", "value=S\n")
                .unwrap()
                .rvs_render_v2(),
        )
        .unwrap();
        rvs_write_migration_marker_BIS(&staging).unwrap();
        rvs_exchange_caps_dirs_BIS(&caps, &staging).unwrap();
        rvs_rename_no_replace_BIS(&staging, &backup).unwrap();

        *RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned") =
            Some((backup.clone(), 1));
        let recovery = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let _ = RVS_SYNC_DIR_FAULT
            .lock()
            .expect("never: sync fault injector mutex should not be poisoned")
            .take();
        let marker_after_failure = rvs_migration_markers_BIS(&caps).unwrap().len();
        let backup_after_failure = backup.exists();
        let retry = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let marker_after_retry = rvs_migration_markers_BIS(&caps).unwrap().len();
        let active = CapsMap::rvs_load_dir_BIS(&caps)
            .ok()
            .and_then(|map| map.rvs_lookup("value").map(CapabilitySet::rvs_letters))
            .unwrap_or_else(|| "<unavailable>".to_string());
        let output = format!(
            "recovery_error={}\nmarker_after_failure={marker_after_failure}\nbackup_after_failure={backup_after_failure}\nretry_ok={}\nmarker_after_retry={marker_after_retry}\nactive={active}\n",
            recovery.is_err(),
            retry.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_backup_sync_failure_keeps_recovery_marker",
            &output,
        );

        assert!(recovery.is_err());
        assert_eq!(marker_after_failure, 1);
        assert!(backup_after_failure);
        assert!(retry.is_ok());
        assert_eq!(marker_after_retry, 0);
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260715_backup_publish_never_replaces_existing_path() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-backup-no-replace");
        let staging = project.join("staging");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&staging).unwrap();
        std::fs::create_dir(&backup).unwrap();
        std::fs::write(staging.join("seed"), "old caps").unwrap();
        std::fs::write(backup.join("sentinel"), "existing backup").unwrap();

        let result = rvs_rename_no_replace_BIS(&staging, &backup);
        let output = format!(
            "error={}\nstaging={}\nbackup={}\n",
            result.is_err(),
            std::fs::read_to_string(staging.join("seed")).unwrap(),
            std::fs::read_to_string(backup.join("sentinel")).unwrap(),
        );
        rvs_snapshot_BIS(
            "test_20260715_backup_publish_never_replaces_existing_path",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_caps_migration_rejects_symlinked_directory() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-symlink");
        let target = project.join("target");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &caps).unwrap();

        let result = rvs_migrate_caps_dir_BIS(&caps, &backup);
        let output = format!("{}\n", result.unwrap_err())
            .replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_caps_migration_rejects_symlinked_directory",
            &output,
        );

        assert!(caps.is_symlink());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_caps_migration_rejects_symlinked_layer() {
        let project = rvs_make_temp_dir_BIS("caps-v1-to-v2-symlink-layer");
        let caps = project.join("caps");
        let backup = project.join("caps.v1-backup");
        let source = project.join("seed-source");
        std::fs::create_dir(&caps).unwrap();
        std::fs::write(&source, "value=S\n").unwrap();
        std::os::unix::fs::symlink(&source, caps.join("seed")).unwrap();

        let error = rvs_migrate_caps_dir_BIS(&caps, &backup).unwrap_err();
        let output = format!("{error}\n").replace(&project.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260715_caps_migration_rejects_symlinked_layer",
            &output,
        );

        assert!(matches!(error, CapsMigrationError::LayerNotFile { .. }));
        std::fs::remove_dir_all(project).unwrap();
    }
}
