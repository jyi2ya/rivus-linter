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
#[allow(
    clippy::allow_attributes,
    reason = "the process-local and POSIX locks must be released during panic unwinding"
)]
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

pub(crate) fn rvs_atomic_write_BIST(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut f = tempfile::NamedTempFile::new_in(
        path.parent()
            .expect("never: atomic write path has a parent"),
    )?;
    f.write_all(content)?;
    f.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn rvs_read_file_BIS(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

pub(crate) fn rvs_read_file_utf8_BIS(path: &Path) -> std::io::Result<String> {
    let bytes = rvs_read_file_BIS(path)?;
    String::from_utf8(bytes).map_err(|source| {
        std::io::Error::other(format!("invalid UTF-8 in {}: {source}", path.display()))
    })
}

pub(crate) fn rvs_validate_optional_dir_BIS(path: &Path, label: &str) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) if path.is_dir() => Ok(true),
        Ok(_) => Err(format!("{label} '{}' is not a directory", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect {label} '{}': {error}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_support::rvs_make_temp_dir_BIST;
    use crate::test_support::rvs_snapshot_BIS;

    #[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn test_20260716_directory_lock_is_not_inherited_by_pre_exec_child() {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixStream;
        use std::os::unix::process::CommandExt as _;

        let dir = rvs_make_temp_dir_BIST("directory-lock-fork-inheritance");
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

        let dir = rvs_make_temp_dir_BIST("directory-lock-other-process");
        let lock = rvs_try_lock_directory_BIS(&dir).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(
                "environment::fs_guard::tests::test_20260716_directory_lock_excludes_other_process",
            )
            .arg("--nocapture")
            .env(CHILD_DIRECTORY_ENV, dir.as_os_str())
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
        let dir = rvs_make_temp_dir_BIST("directory-lock-symlink");
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
}
