use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
const RVS_DIRECTORY_LOCK_FILE: &str = ".rivus-caps.lock";

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
static RVS_DIRECTORY_LOCKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeSet<PathBuf>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
#[derive(Debug)]
pub(crate) struct RivusDirectoryLock {
    file: Option<std::fs::File>,
    directory: PathBuf,
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_vendor = "apple")))]
#[derive(Debug)]
pub(crate) struct RivusDirectoryLock;

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
pub(crate) fn rvs_try_lock_directory_BIS(directory: &Path) -> std::io::Result<RivusDirectoryLock> {
    let directory = directory.canonicalize()?;
    {
        let mut locked = RVS_DIRECTORY_LOCKS.lock().map_err(|error| {
            std::io::Error::other(format!("directory lock registry is poisoned: {error}"))
        })?;
        if !locked.insert(directory.clone()) {
            return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        }
    }

    let lock_result = (|| {
        let fd = rustix::fs::open(
            directory.join(RVS_DIRECTORY_LOCK_FILE),
            rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(std::io::Error::from)?;
        let file = std::fs::File::from(fd);
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} must be a regular lock file",
                    directory.join(RVS_DIRECTORY_LOCK_FILE).display()
                ),
            ));
        }
        match rustix::fs::fcntl_lock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::ACCESS) => {
                return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
            }
            Err(error) => return Err(std::io::Error::from(error)),
        }
        Ok(file)
    })();

    match lock_result {
        Ok(file) => Ok(RivusDirectoryLock {
            file: Some(file),
            directory,
        }),
        Err(error) => {
            if let Ok(mut locked) = RVS_DIRECTORY_LOCKS.lock() {
                locked.remove(&directory);
            }
            Err(error)
        }
    }
}

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
impl Drop for RivusDirectoryLock {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Ok(mut locked) = RVS_DIRECTORY_LOCKS.lock() {
            locked.remove(&self.directory);
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_vendor = "apple")))]
pub(crate) fn rvs_try_lock_directory_BIS(_directory: &Path) -> std::io::Result<RivusDirectoryLock> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "directory locking is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn rvs_open_regular_file_no_follow_BIS(path: &Path) -> std::io::Result<std::fs::File> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file = std::fs::File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
pub(crate) fn rvs_read_regular_file_no_follow_BIS(path: &Path) -> std::io::Result<String> {
    let mut file = rvs_open_regular_file_no_follow_BIS(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

#[cfg(not(unix))]
pub(crate) fn rvs_read_regular_file_no_follow_BIS(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    std::fs::read_to_string(path)
}

#[cfg(unix)]
pub(crate) fn rvs_sync_regular_file_no_follow_BIS(path: &Path) -> std::io::Result<()> {
    rvs_open_regular_file_no_follow_BIS(path)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn rvs_sync_regular_file_no_follow_BIS(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    std::fs::File::open(path)?.sync_all()
}

#[cfg(unix)]
pub(crate) fn rvs_set_permissions_no_follow_BIS(
    path: &Path,
    permissions: std::fs::Permissions,
) -> std::io::Result<()> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    std::fs::File::from(fd).set_permissions(permissions)
}

#[cfg(not(unix))]
pub(crate) fn rvs_set_permissions_no_follow_BIS(
    path: &Path,
    permissions: std::fs::Permissions,
) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} must not be a symlink", path.display()),
        ));
    }
    std::fs::File::open(path)?.set_permissions(permissions)
}

#[derive(Debug)]
pub(crate) enum AtomicWriteFailureKind {
    Inspect {
        path: PathBuf,
        source: std::io::Error,
    },
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    CreateExhausted,
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    SetPermissions {
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
        AtomicWriteFailureKind::Inspect { path, source } => format!(
            "cannot inspect {}: {source}",
            rvs_format_atomic_write_path(&path, quote_paths)
        ),
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
        AtomicWriteFailureKind::SetPermissions { path, source } => format!(
            "cannot preserve permissions on {}: {source}",
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
    let existing_permissions = match std::fs::symlink_metadata(final_path) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(AtomicWriteFailure {
                kind: AtomicWriteFailureKind::Inspect {
                    path: final_path.to_path_buf(),
                    source,
                },
                cleanup_error: None,
            });
        }
    };
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
        .and_then(|()| match existing_permissions {
            Some(permissions) => file.set_permissions(permissions).map_err(|source| {
                AtomicWriteFailureKind::SetPermissions {
                    path: temp_path.clone(),
                    source,
                }
            }),
            None => Ok(()),
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
    #[cfg(unix)]
    use crate::test_support::rvs_make_temp_dir_BIS;
    use crate::test_support::rvs_snapshot_BIS;

    #[cfg(unix)]
    #[test]
    fn test_20260716_atomic_write_preserves_existing_regular_file_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = rvs_make_temp_dir_BIS("atomic-write-preserves-permissions");
        let path = dir.join("output");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let temp_path_for_attempt = |attempt| rvs_atomic_sibling_temp_path_S(&path, attempt);

        let result = rvs_write_atomic_BIS(&path, b"new", &temp_path_for_attempt);
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let content = std::fs::read_to_string(&path).unwrap();
        let output = format!(
            "result_is_ok={}\nmode={mode:o}\ncontent={content:?}\n",
            result.is_ok(),
        );
        rvs_snapshot_BIS(
            "test_20260716_atomic_write_preserves_existing_regular_file_permissions",
            &output,
        );

        assert!(result.is_ok());
        assert_eq!(mode, 0o700);
        assert_eq!(content, "new");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260716_set_permissions_does_not_follow_symlink() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = rvs_make_temp_dir_BIS("permissions-no-follow");
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::write(&target, "value").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result =
            rvs_set_permissions_no_follow_BIS(&link, std::fs::Permissions::from_mode(0o644));
        let target_mode = std::fs::symlink_metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let output = format!(
            "error={}\ntarget_mode={target_mode:o}\nlink_is_symlink={}\n",
            result.is_err(),
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
        );
        rvs_snapshot_BIS(
            "test_20260716_set_permissions_does_not_follow_symlink",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(target_mode, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_directory_lock_is_not_inherited_by_pre_exec_child() {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixStream;
        use std::os::unix::process::CommandExt as _;

        let dir = rvs_make_temp_dir_BIS("directory-lock-fork-inheritance");
        let lock = rvs_try_lock_directory_BIS(&dir).unwrap();
        let lock_path = dir.join(RVS_DIRECTORY_LOCK_FILE);
        let lock_file_regular = std::fs::symlink_metadata(&lock_path)
            .unwrap()
            .file_type()
            .is_file();
        let (mut parent_socket, child_socket) = UnixStream::pair().unwrap();
        parent_socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let executable = std::env::current_exe().unwrap();

        let (ready, retry, child_success) = std::thread::scope(|scope| {
            let child = scope.spawn(move || {
                let mut command = std::process::Command::new(executable);
                command.arg("--list");
                // SAFETY: the child closure only performs async-signal-safe read/write syscalls.
                unsafe {
                    command.pre_exec(move || {
                        let _ =
                            rustix::io::write(&child_socket, &[1]).map_err(std::io::Error::from)?;
                        let mut release = [0u8; 1];
                        let _ = rustix::io::read(&child_socket, &mut release)
                            .map_err(std::io::Error::from)?;
                        Ok(())
                    });
                }
                command.status()
            });

            let mut ready = [0u8; 1];
            parent_socket.read_exact(&mut ready).unwrap();
            drop(lock);
            let retry = rvs_try_lock_directory_BIS(&dir);
            parent_socket.write_all(&[1]).unwrap();
            let child_success = child
                .join()
                .expect("never: pre-exec lock test child thread should not panic")
                .unwrap()
                .success();
            (ready[0], retry, child_success)
        });
        let retry_is_ok = retry.is_ok();
        drop(retry);
        let output = format!(
            "ready={ready}\nretry_ok={retry_is_ok}\nchild_success={child_success}\nlock_file_regular={lock_file_regular}\n"
        );
        rvs_snapshot_BIS(
            "test_20260716_directory_lock_is_not_inherited_by_pre_exec_child",
            &output,
        );

        assert_eq!(ready, 1);
        assert!(retry_is_ok);
        assert!(child_success);
        assert!(lock_file_regular);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_directory_lock_excludes_other_process() {
        const CHILD_DIRECTORY_ENV: &str = "RVS_TEST_DIRECTORY_LOCK_CHILD";

        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY_ENV) {
            let error = rvs_try_lock_directory_BIS(Path::new(&directory)).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
            return;
        }

        let dir = rvs_make_temp_dir_BIS("directory-lock-other-process");
        let lock = rvs_try_lock_directory_BIS(&dir).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("fs_guard::tests::test_20260716_directory_lock_excludes_other_process")
            .arg("--nocapture")
            .env(CHILD_DIRECTORY_ENV, &dir)
            .status()
            .unwrap();
        drop(lock);
        let retry = rvs_try_lock_directory_BIS(&dir);
        let retry_is_ok = retry.is_ok();
        drop(retry);
        let output = format!(
            "child_success={}\nretry_ok={retry_is_ok}\n",
            child.success(),
        );
        rvs_snapshot_BIS(
            "test_20260716_directory_lock_excludes_other_process",
            &output,
        );

        assert!(child.success());
        assert!(retry_is_ok);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260727_directory_lock_rejects_symlink() {
        let dir = rvs_make_temp_dir_BIS("directory-lock-symlink");
        let victim = dir.join("victim");
        let lock_path = dir.join(RVS_DIRECTORY_LOCK_FILE);
        std::fs::write(&victim, "safe").unwrap();
        std::os::unix::fs::symlink(&victim, &lock_path).unwrap();

        let result = rvs_try_lock_directory_BIS(&dir);
        let victim_content = std::fs::read_to_string(&victim).unwrap();
        let lock_is_symlink = std::fs::symlink_metadata(&lock_path)
            .unwrap()
            .file_type()
            .is_symlink();
        std::fs::remove_file(&lock_path).unwrap();
        let retry = rvs_try_lock_directory_BIS(&dir);
        let retry_is_ok = retry.is_ok();
        drop(retry);
        let output = format!(
            "result_is_err={}\nvictim={victim_content:?}\nlock_is_symlink={lock_is_symlink}\nretry_ok={retry_is_ok}\n",
            result.is_err(),
        );
        rvs_snapshot_BIS("test_20260727_directory_lock_rejects_symlink", &output);

        assert!(result.is_err());
        assert_eq!(victim_content, "safe");
        assert!(lock_is_symlink);
        assert!(retry_is_ok);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", target_vendor = "apple")))]
    #[test]
    fn test_20260716_unsupported_directory_lock_returns_unsupported() {
        let error = rvs_try_lock_directory_BIS(Path::new(".")).unwrap_err();
        let output = format!("kind={:?}\n", error.kind());
        rvs_snapshot_BIS(
            "test_20260716_unsupported_directory_lock_returns_unsupported",
            &output,
        );

        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    }

    #[derive(Clone, Copy, Debug)]
    enum AtomicWriteFailureCase {
        Inspect,
        Open,
        CreateExhausted,
        Write,
        SetPermissions,
        Sync,
        Rename,
    }

    fn rvs_failure(case: AtomicWriteFailureCase, cleanup_error: bool) -> AtomicWriteFailure {
        let kind = match case {
            AtomicWriteFailureCase::Inspect => AtomicWriteFailureKind::Inspect {
                path: PathBuf::from("/workspace/output"),
                source: std::io::Error::other("inspect failed"),
            },
            AtomicWriteFailureCase::Open => AtomicWriteFailureKind::Open {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("open failed"),
            },
            AtomicWriteFailureCase::CreateExhausted => AtomicWriteFailureKind::CreateExhausted,
            AtomicWriteFailureCase::Write => AtomicWriteFailureKind::Write {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("write failed"),
            },
            AtomicWriteFailureCase::SetPermissions => AtomicWriteFailureKind::SetPermissions {
                path: PathBuf::from("/workspace/.output.tmp"),
                source: std::io::Error::other("permission preservation failed"),
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
                "inspect",
                AtomicWriteFailureCase::Inspect,
                "/workspace/output",
                "temp file",
                false,
                false,
            ),
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
                "permissions",
                AtomicWriteFailureCase::SetPermissions,
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
