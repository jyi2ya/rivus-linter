use std::path::Path;

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
