use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) enum AtomicWriteFailureKind {
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    CreateExhausted,
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

fn rvs_format_atomic_write_path(path: &Path, quote_paths: bool) -> String {
    if quote_paths {
        format!("'{}'", path.display())
    } else {
        path.display().to_string()
    }
}

pub(crate) fn rvs_atomic_sibling_temp_path_S(final_path: &Path, attempt: usize) -> PathBuf {
    debug_assert!(attempt < 100, "atomic temp filename retry bound");
    let file_name = final_path
        .file_name()
        .expect("never: atomic final path has a file name")
        .to_string_lossy();
    final_path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        attempt
    ))
}

pub(crate) fn rvs_is_atomic_sibling_temp_name(name: &str) -> bool {
    let Some(without_tmp) = name.strip_suffix(".tmp") else {
        return false;
    };
    let Some((without_attempt, attempt)) = without_tmp.rsplit_once('.') else {
        return false;
    };
    let Some((file_name, pid)) = without_attempt.rsplit_once('.') else {
        return false;
    };
    file_name.starts_with('.')
        && file_name.len() > 1
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !attempt.is_empty()
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn rvs_render_atomic_write_failure(
    failure: AtomicWriteFailure,
    final_path: &Path,
    create_exhausted_noun: &str,
    quote_paths: bool,
) -> String {
    debug_assert!(
        !final_path.as_os_str().is_empty(),
        "atomic write final path must not be empty"
    );
    debug_assert!(
        !create_exhausted_noun.is_empty(),
        "atomic write collision noun must not be empty"
    );
    let AtomicWriteFailure {
        kind,
        cleanup_error,
    } = failure;
    let message = match kind {
        AtomicWriteFailureKind::Open { path, source } => format!(
            "cannot create {}: {source}",
            rvs_format_atomic_write_path(&path, quote_paths)
        ),
        AtomicWriteFailureKind::CreateExhausted => format!(
            "cannot create {} for {}: too many collisions",
            create_exhausted_noun,
            rvs_format_atomic_write_path(final_path, quote_paths)
        ),
        AtomicWriteFailureKind::Write { path, source } => format!(
            "cannot write {}: {source}",
            rvs_format_atomic_write_path(&path, quote_paths)
        ),
        AtomicWriteFailureKind::Sync { path, source } => format!(
            "cannot sync {}: {source}",
            rvs_format_atomic_write_path(&path, quote_paths)
        ),
        AtomicWriteFailureKind::Rename {
            temp_path,
            final_path,
            source,
        } => format!(
            "cannot rename {} to {}: {source}",
            rvs_format_atomic_write_path(&temp_path, quote_paths),
            rvs_format_atomic_write_path(&final_path, quote_paths)
        ),
    };
    match cleanup_error {
        Some(cleanup_error) => {
            format!("{message}; additionally cannot remove temp file: {cleanup_error}")
        }
        None => message,
    }
}

pub(crate) fn rvs_write_atomic_BIS(
    final_path: &Path,
    content: &[u8],
    temp_path_for_attempt: &impl Fn(usize) -> PathBuf,
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
                    kind: AtomicWriteFailureKind::Open {
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
            kind: AtomicWriteFailureKind::CreateExhausted,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[derive(Clone, Copy, Debug)]
    enum AtomicWriteFailureCase {
        Open,
        CreateExhausted,
        Write,
        Sync,
        Rename,
    }

    fn rvs_failure(case: AtomicWriteFailureCase, cleanup_error: bool) -> AtomicWriteFailure {
        let kind = match case {
            AtomicWriteFailureCase::Open => AtomicWriteFailureKind::Open {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("open failed"),
            },
            AtomicWriteFailureCase::CreateExhausted => AtomicWriteFailureKind::CreateExhausted,
            AtomicWriteFailureCase::Write => AtomicWriteFailureKind::Write {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("write failed"),
            },
            AtomicWriteFailureCase::Sync => AtomicWriteFailureKind::Sync {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("sync failed"),
            },
            AtomicWriteFailureCase::Rename => AtomicWriteFailureKind::Rename {
                temp_path: PathBuf::from("/workspace/.output.tmp"),
                final_path: PathBuf::from("/workspace/output"),
                source: std::io::Error::other("rename failed"),
            },
        };
        AtomicWriteFailure {
            kind,
            cleanup_error: cleanup_error.then(|| std::io::Error::other("cleanup failed")),
        }
    }

    #[test]
    fn test_20260710_atomic_write_failure_rendering_table() {
        let cases = [
            (
                "setup-open",
                AtomicWriteFailureCase::Open,
                "/workspace/Cargo.toml",
                "temp file",
                true,
                false,
            ),
            (
                "setup-collision",
                AtomicWriteFailureCase::CreateExhausted,
                "/workspace/Cargo.toml",
                "temp file",
                true,
                false,
            ),
            (
                "capsmap-collision",
                AtomicWriteFailureCase::CreateExhausted,
                "/workspace/caps/deps",
                "temp capsmap file",
                false,
                false,
            ),
            (
                "artifact-collision",
                AtomicWriteFailureCase::CreateExhausted,
                "/workspace/target/rivus-callgraph/demo.json",
                "temp artifact",
                false,
                false,
            ),
            (
                "write",
                AtomicWriteFailureCase::Write,
                "/workspace/output",
                "temp file",
                false,
                false,
            ),
            (
                "sync-quoted",
                AtomicWriteFailureCase::Sync,
                "/workspace/output",
                "temp file",
                true,
                false,
            ),
            (
                "rename",
                AtomicWriteFailureCase::Rename,
                "/workspace/output",
                "temp file",
                false,
                false,
            ),
            (
                "rename-cleanup",
                AtomicWriteFailureCase::Rename,
                "/workspace/output",
                "temp file",
                false,
                true,
            ),
        ];
        let mut output = String::new();
        for (label, failure_case, final_path, noun, quote_paths, cleanup_error) in cases {
            output.push_str(&format!(
                "[{label}]\n{}\n",
                rvs_render_atomic_write_failure(
                    rvs_failure(failure_case, cleanup_error),
                    Path::new(final_path),
                    noun,
                    quote_paths,
                )
            ));
        }

        rvs_snapshot_BIS(
            "test_20260710_atomic_write_failure_rendering_table",
            &output,
        );
        assert_eq!(
            output,
            include_str!("../test_out/test_20260710_atomic_write_failure_rendering_table.out")
        );
    }
}
