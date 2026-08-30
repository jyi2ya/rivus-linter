use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::capsmap::{
    CapsMap, CapsMapError, LAYER_ORDER, rvs_load_distributed_seed, rvs_reserved_layer_name,
    rvs_sort_by_layer_M,
};

use super::fs_guard;

#[derive(Debug, Clone, Copy)]
enum CapsDirSelection<'a> {
    All,
    Include(&'a [&'a str]),
    Exclude(&'a [&'a OsStr]),
}

#[derive(Debug, Clone, Copy)]
enum DistributedSeedSelection {
    #[cfg(test)]
    Omit,
    Include,
}

impl CapsMap {
    /// 加载目录中所有 caps 文件，按固定层级顺序合并。
    ///
    /// 层级顺序：std → seed → deps → ext → suppress；分发 seed（如选择
    /// 加载）合并在最底层。目录只支持这五个文件。
    /// 后加载的覆盖先加载的同名条目。
    #[cfg(test)]
    pub fn rvs_load_dir_BIS(dir: &Path) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(dir, CapsDirSelection::All, DistributedSeedSelection::Omit)
    }

    /// Load effective capability data with the distributed seed merged at
    /// the very bottom of the layer order. The distribution transport is
    /// intentionally hidden so callers do not depend on the seed being
    /// embedded in the binary.
    pub fn rvs_load_effective_dir_BIS(dir: &Path) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(
            dir,
            CapsDirSelection::All,
            DistributedSeedSelection::Include,
        )
    }

    /// 加载目录中指定的层级子集。
    /// 例如 `&["seed", "suppress"]` 只加载这两个文件。
    #[cfg(test)]
    pub fn rvs_load_dir_layers_BIS(dir: &Path, layers: &[&str]) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(
            dir,
            CapsDirSelection::Include(layers),
            DistributedSeedSelection::Omit,
        )
    }

    pub(crate) fn rvs_load_effective_dir_layers_BIS(
        dir: &Path,
        layers: &[&str],
    ) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(
            dir,
            CapsDirSelection::Include(layers),
            DistributedSeedSelection::Include,
        )
    }

    /// 加载目录中除指定层级外的所有文件。
    /// 例如 `&["deps"]` 加载 std/seed/suppress/ext 但不加载 deps。
    #[cfg(test)]
    pub fn rvs_load_dir_excluding_BIS(dir: &Path, exclude: &[&str]) -> Result<Self, CapsMapError> {
        let exclude = exclude.iter().map(OsStr::new).collect::<Vec<_>>();
        rvs_load_caps_dir_BIS(
            dir,
            CapsDirSelection::Exclude(&exclude),
            DistributedSeedSelection::Omit,
        )
    }

    pub(crate) fn rvs_load_effective_dir_excluding_names_BIS(
        dir: &Path,
        exclude: &[&OsStr],
    ) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(
            dir,
            CapsDirSelection::Exclude(exclude),
            DistributedSeedSelection::Include,
        )
    }

    /// Test-facing raw loader for validating path rejection independently of
    /// distributed capability data.
    #[cfg(test)]
    pub fn rvs_load_BIS(path: &Path) -> Result<Self, CapsMapError> {
        if path.is_dir() {
            Self::rvs_load_dir_BIS(path)
        } else {
            Err(CapsMapError::PathMustBeDirectory {
                path: path.display().to_string(),
            })
        }
    }

    pub(crate) fn rvs_load_effective_BIS(path: &Path) -> Result<Self, CapsMapError> {
        if path.is_dir() {
            Self::rvs_load_effective_dir_BIS(path)
        } else {
            Err(CapsMapError::PathMustBeDirectory {
                path: path.display().to_string(),
            })
        }
    }
}

fn rvs_load_caps_dir_BIS(
    dir: &Path,
    selection: CapsDirSelection<'_>,
    distributed_seed_selection: DistributedSeedSelection,
) -> Result<CapsMap, CapsMapError> {
    let selection = rvs_check_caps_dir_selection(dir, selection)?;
    // The distributed seed merges at the very bottom of the layer order,
    // beneath every project layer (including generated std): generated
    // records override the curated baseline, and the hand-curated
    // project seed layer sits above both.
    let mut result = match distributed_seed_selection {
        DistributedSeedSelection::Include if rvs_selection_contains_seed(selection) => {
            rvs_load_distributed_seed()?
        }
        #[cfg(test)]
        DistributedSeedSelection::Omit | DistributedSeedSelection::Include => CapsMap::rvs_new(),
        #[cfg(not(test))]
        DistributedSeedSelection::Include => CapsMap::rvs_new(),
    };
    if !rvs_caps_dir_exists_BIS(dir)? {
        return Ok(result);
    }
    rvs_require_caps_dir_BIS(dir)?;
    let mut files = rvs_collect_selected_caps_dir_files_BIS(dir, selection)?;
    rvs_sort_by_layer_M(&mut files);
    for path in files {
        if matches!(selection, CapsDirSelection::Include(_))
            && !rvs_optional_caps_layer_file_BIS(&path)?
        {
            continue;
        }
        let content =
            fs_guard::rvs_read_file_utf8_BIS(&path).map_err(|error| CapsMapError::FileRead {
                path: path.display().to_string(),
                error: error.to_string(),
            })?;
        let partial = rvs_parse_caps_file(&path, &content)?;
        result.rvs_extend_from_M(partial);
    }
    Ok(result)
}

fn rvs_selection_contains_seed(selection: CapsDirSelection<'_>) -> bool {
    match selection {
        CapsDirSelection::All => true,
        CapsDirSelection::Include(layers) => layers.contains(&"seed"),
        CapsDirSelection::Exclude(layers) => !layers.contains(&OsStr::new("seed")),
    }
}

pub(crate) fn rvs_parse_caps_file(path: &Path, content: &str) -> Result<CapsMap, CapsMapError> {
    let layer = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("<unknown>");
    CapsMap::rvs_parse_with_source(content, Some((layer, path))).map_err(|source| {
        CapsMapError::FileParse {
            path: path.display().to_string(),
            source: Box::new(source),
        }
    })
}

pub(crate) fn rvs_caps_layer_file_path(dir: &Path, layer: &str) -> Result<PathBuf, CapsMapError> {
    let mut components = Path::new(layer).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(CapsMapError::InvalidLayerName {
            layer: layer.to_string(),
        });
    }
    if let Some(expected) = rvs_reserved_layer_name(OsStr::new(layer))
        && layer != expected
    {
        return Err(CapsMapError::NonCanonicalLayerName {
            layer: layer.to_string(),
            expected,
        });
    }
    Ok(dir.join(layer))
}

fn rvs_check_caps_dir_selection<'a>(
    dir: &Path,
    selection: CapsDirSelection<'a>,
) -> Result<CapsDirSelection<'a>, CapsMapError> {
    if let CapsDirSelection::Include(layers) = selection {
        for layer in layers {
            let _ = rvs_caps_layer_file_path(dir, layer)?;
        }
    }
    Ok(selection)
}

fn rvs_collect_selected_caps_dir_files_BIS(
    dir: &Path,
    selection: CapsDirSelection<'_>,
) -> Result<Vec<PathBuf>, CapsMapError> {
    match selection {
        CapsDirSelection::All => rvs_collect_caps_dir_files_BIS(dir, &[]),
        CapsDirSelection::Include(layers) => {
            let _ = rvs_collect_caps_dir_files_BIS(dir, &[])?;
            layers
                .iter()
                .map(|layer| rvs_caps_layer_file_path(dir, layer))
                .collect()
        }
        CapsDirSelection::Exclude(exclude) => rvs_collect_caps_dir_files_BIS(dir, exclude),
    }
}

fn rvs_collect_caps_dir_files_BIS(
    dir: &Path,
    exclude: &[&OsStr],
) -> Result<Vec<PathBuf>, CapsMapError> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|error| CapsMapError::DirRead {
        message: format!("{}: {error}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| CapsMapError::DirRead {
            message: format!("{}: {error}", dir.display()),
        })?;
        let file_type = entry.file_type().map_err(|error| CapsMapError::DirRead {
            message: format!("{}: {error}", entry.path().display()),
        })?;
        let path = entry.path();
        let raw_name = path.file_name().unwrap_or_else(|| OsStr::new(""));
        if exclude.contains(&raw_name) {
            continue;
        }
        let name = raw_name.to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if let Some(expected) = rvs_reserved_layer_name(raw_name)
            && name != expected
        {
            return Err(CapsMapError::NonCanonicalLayerName {
                layer: name,
                expected,
            });
        }
        if file_type.is_symlink() {
            return Err(CapsMapError::PathMustBeFile {
                path: path.display().to_string(),
            });
        }
        if file_type.is_file() {
            // Only the five canonical layers are supported; stray files in
            // caps/ are rejected so typos cannot silently become unread
            // overlays that later layers override anyway.
            if !LAYER_ORDER.contains(&name.as_str()) {
                return Err(CapsMapError::UnsupportedLayerName { layer: name });
            }
            files.push(path);
        } else if LAYER_ORDER.contains(&name.as_str()) {
            return Err(CapsMapError::PathMustBeFile {
                path: path.display().to_string(),
            });
        }
    }
    Ok(files)
}

pub(crate) fn rvs_require_caps_dir_BIS(dir: &Path) -> Result<(), CapsMapError> {
    if dir.is_dir() {
        Ok(())
    } else {
        Err(CapsMapError::PathMustBeDirectory {
            path: dir.display().to_string(),
        })
    }
}

fn rvs_caps_dir_exists_BIS(dir: &Path) -> Result<bool, CapsMapError> {
    match std::fs::metadata(dir) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(dir) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Ok(_) => Err(CapsMapError::PathMustBeDirectory {
                    path: dir.display().to_string(),
                }),
                Err(symlink_error) => Err(CapsMapError::DirRead {
                    message: format!("{}: {symlink_error}", dir.display()),
                }),
            }
        }
        Err(error) => Err(CapsMapError::DirRead {
            message: format!("{}: {error}", dir.display()),
        }),
    }
}

fn rvs_optional_caps_layer_file_BIS(path: &Path) -> Result<bool, CapsMapError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(CapsMapError::PathMustBeFile {
            path: path.display().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Ok(_) => Err(CapsMapError::PathMustBeFile {
                    path: path.display().to_string(),
                }),
                Err(symlink_error) => Err(CapsMapError::FileRead {
                    path: path.display().to_string(),
                    error: symlink_error.to_string(),
                }),
            }
        }
        Err(error) => Err(CapsMapError::FileRead {
            path: path.display().to_string(),
            error: error.to_string(),
        }),
    }
}
