use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) enum AtomicWriteFailureKind {
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    TooManyCollisions,
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Sync {
        path: PathBuf,
        source: std::io::Error,
    },
    Rename {
        temp_path: PathBuf,
        final_path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug)]
pub(crate) struct AtomicWriteFailure {
    pub(crate) kind: AtomicWriteFailureKind,
    pub(crate) cleanup_error: Option<std::io::Error>,
}

pub(crate) fn rvs_write_atomic_BIS(
    final_path: &Path,
    content: &[u8],
    mut temp_path_for_attempt: impl FnMut(usize) -> PathBuf,
) -> Result<(), AtomicWriteFailure> {
    let mut temp_path = None;
    let mut file = None;
    for attempt in 0..100usize {
        debug_assert!(attempt < 100, "temp filename retry bound");
        let candidate = temp_path_for_attempt(attempt);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(opened) => {
                temp_path = Some(candidate);
                file = Some(opened);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(AtomicWriteFailure {
                    kind: AtomicWriteFailureKind::Create {
                        path: candidate,
                        source,
                    },
                    cleanup_error: None,
                });
            }
        }
    }

    let Some(temp_path) = temp_path else {
        return Err(AtomicWriteFailure {
            kind: AtomicWriteFailureKind::TooManyCollisions,
            cleanup_error: None,
        });
    };
    let mut file = file.expect("never: temp file handle set when temp path exists");
    let operation = file
        .write_all(content)
        .map_err(|source| AtomicWriteFailureKind::Write {
            path: temp_path.clone(),
            source,
        })
        .and_then(|()| {
            file.sync_all()
                .map_err(|source| AtomicWriteFailureKind::Sync {
                    path: temp_path.clone(),
                    source,
                })
        });
    drop(file);
    let operation = operation.and_then(|()| {
        std::fs::rename(&temp_path, final_path).map_err(|source| AtomicWriteFailureKind::Rename {
            temp_path: temp_path.clone(),
            final_path: final_path.to_path_buf(),
            source,
        })
    });
    if let Err(kind) = operation {
        let cleanup_error = std::fs::remove_file(&temp_path).err();
        return Err(AtomicWriteFailure {
            kind,
            cleanup_error,
        });
    }
    Ok(())
}

pub(crate) fn rvs_validate_optional_dir_BIS(path: &Path, label: &str) -> Result<bool, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(format!("{label} must be a directory: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match std::fs::symlink_metadata(path)
        {
            Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Ok(_) => Err(format!("{label} must be a directory: {}", path.display())),
            Err(symlink_error) => Err(format!(
                "cannot inspect {label} {}: {symlink_error}",
                path.display()
            )),
        },
        Err(e) => Err(format!("cannot inspect {label} {}: {e}", path.display())),
    }
}
