use std::path::Path;

use walkdir::WalkDir;

/// 一个源文件：路径与内容。
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
}

/// 从路径出发，遍历目录，读尽所有 .rs 文件。
/// 阻塞 + 读写：与文件系统打交道，免不了等。
///
/// 千里之行，始于足下；
/// 万卷之源，皆从磁盘来。
#[allow(non_snake_case)]
pub fn rvs_read_rust_sources_BI(path: &Path) -> Result<Vec<SourceFile>, ReadError> {
    debug_assert!(path.exists(), "路径必须存在");

    let file_paths = if path.is_dir() {
        WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
            .map(|e| e.into_path())
            .collect::<Vec<_>>()
    } else {
        vec![path.to_path_buf()]
    };

    let mut sources = Vec::new();
    for file_path in file_paths {
        let source = std::fs::read_to_string(&file_path).map_err(|e| ReadError::FileRead {
            path: file_path.display().to_string(),
            source: e,
        })?;
        sources.push(SourceFile {
            path: file_path.display().to_string(),
            source,
        });
    }

    Ok(sources)
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("failed to read file '{path}': {source}")]
    FileRead {
        path: String,
        source: std::io::Error,
    },
}

/// 从路径读取所有 `.mir` 文件。路径可为单个 mir 文件或目录（递归扫描）。
#[allow(non_snake_case)]
pub fn rvs_read_mir_sources_BI(path: &Path) -> Result<Vec<SourceFile>, ReadError> {
    debug_assert!(path.exists(), "路径必须存在");

    let file_paths = if path.is_dir() {
        WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "mir"))
            .map(|e| e.into_path())
            .collect::<Vec<_>>()
    } else {
        vec![path.to_path_buf()]
    };

    let mut sources = Vec::new();
    for file_path in file_paths {
        let source = std::fs::read_to_string(&file_path).map_err(|e| ReadError::FileRead {
            path: file_path.display().to_string(),
            source: e,
        })?;
        sources.push(SourceFile {
            path: file_path.display().to_string(),
            source,
        });
    }

    Ok(sources)
}
