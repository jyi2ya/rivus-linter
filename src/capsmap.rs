use std::collections::BTreeMap;
use std::path::Path;

use crate::capability::{CapabilityParseError, CapabilitySet};

/// 能力之鉴：非 rvs 函数的品行录。
/// 外人虽无 rvs 前缀，登记在册，亦知其能。
#[derive(Debug, Clone, Default)]
pub struct CapsMap {
    entries: BTreeMap<String, CapabilitySet>,
}

#[derive(Debug, thiserror::Error)]
pub enum CapsMapError {
    #[error("line {line}: invalid capability string '{caps}' for '{key}'")]
    InvalidCaps {
        key: String,
        caps: String,
        line: usize,
        #[source]
        source: CapabilityParseError,
    },
    #[error("line {line}: missing '=' separator")]
    MissingSeparator { line: usize },
    #[error("cannot read caps directory: {0}")]
    DirRead(String),
    #[error("cannot read {path}: {error}")]
    FileRead { path: String, error: String },
}

/// 固定层级顺序。后加载的覆盖先加载的。
/// 这是整个系统中唯一的层级定义——所有调用者都引用这一个常量。
const LAYER_ORDER: &[&str] = &["std", "deps", "seed", "suppress", "ext"];

impl CapsMap {
    /// 构造一个空的能力映射表。
    pub fn rvs_new() -> Self {
        Self::default()
    }

    /// 解析文本为 capsmap。
    ///
    /// 格式：每行 `key=caps` 或 `key=`（表示纯函数）。
    /// 注释以 `#` 开头，但仅从 `=` 之后的值部分剥离——
    /// 键中可含 `#`（如 `closure#0`），因此不从键中剥离注释。
    pub fn rvs_parse(content: &str) -> Result<Self, CapsMapError> {
        let mut entries = BTreeMap::new();
        for (i, raw_line) in content.lines().enumerate() {
            let line_num = i + 1;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (key, value) = trimmed
                .split_once('=')
                .ok_or(CapsMapError::MissingSeparator { line: line_num })?;
            let key = key.trim().to_string();
            let value = value.split('#').next().unwrap_or("").trim();
            let caps =
                CapabilitySet::rvs_from_str(value).map_err(|e| CapsMapError::InvalidCaps {
                    key: key.clone(),
                    caps: value.to_string(),
                    line: line_num,
                    source: e,
                })?;
            entries.insert(key, caps);
        }
        Ok(Self { entries })
    }

    /// 精确匹配查找，不做后缀匹配。
    pub fn rvs_lookup(&self, name: &str) -> Option<&CapabilitySet> {
        self.entries.get(name)
    }

    /// 合并另一个 capsmap，后者覆盖前者。
    pub(crate) fn rvs_extend_from_M(&mut self, other: Self) {
        for (key, caps) in other.entries {
            self.entries.insert(key, caps);
        }
    }

    /// 加载目录中所有 caps 文件，按固定层级顺序合并。
    ///
    /// 层级顺序：std → deps → seed → suppress → ext → 其余按字母序。
    /// 后加载的覆盖先加载的同名条目。
    pub fn rvs_load_dir_BIS(dir: &Path) -> Result<Self, CapsMapError> {
        let mut result = Self::rvs_new();
        if !dir.is_dir() {
            return Ok(result);
        }
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        let entries = std::fs::read_dir(dir)
            .map_err(|e| CapsMapError::DirRead(format!("{}: {e}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| CapsMapError::DirRead(format!("{}: {e}", dir.display())))?;
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            }
        }
        rvs_sort_by_layer_M(&mut files);
        for path in &files {
            let content = std::fs::read_to_string(path).map_err(|e| CapsMapError::FileRead {
                path: path.display().to_string(),
                error: e.to_string(),
            })?;
            let partial = Self::rvs_parse(&content)?;
            result.rvs_extend_from_M(partial);
        }
        Ok(result)
    }

    /// 加载目录中指定的层级子集。
    /// 例如 `&["seed", "suppress"]` 只加载这两个文件。
    pub fn rvs_load_dir_layers_BIS(dir: &Path, layers: &[&str]) -> Result<Self, CapsMapError> {
        let mut result = Self::rvs_new();
        for &layer in layers {
            let path = dir.join(layer);
            if !path.is_file() {
                continue;
            }
            let content = std::fs::read_to_string(&path).map_err(|e| CapsMapError::FileRead {
                path: path.display().to_string(),
                error: e.to_string(),
            })?;
            let partial = Self::rvs_parse(&content)?;
            result.rvs_extend_from_M(partial);
        }
        Ok(result)
    }

    /// 加载目录中除指定层级外的所有文件。
    /// 例如 `&["deps"]` 加载 std/seed/suppress/ext 但不加载 deps。
    pub fn rvs_load_dir_excluding_BIS(dir: &Path, exclude: &[&str]) -> Result<Self, CapsMapError> {
        let mut result = Self::rvs_new();
        if !dir.is_dir() {
            return Ok(result);
        }
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        let entries = std::fs::read_dir(dir)
            .map_err(|e| CapsMapError::DirRead(format!("{}: {e}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| CapsMapError::DirRead(format!("{}: {e}", dir.display())))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if exclude.contains(&name.as_str()) {
                continue;
            }
            files.push(path);
        }
        rvs_sort_by_layer_M(&mut files);
        for path in &files {
            let content = std::fs::read_to_string(path).map_err(|e| CapsMapError::FileRead {
                path: path.display().to_string(),
                error: e.to_string(),
            })?;
            let partial = Self::rvs_parse(&content)?;
            result.rvs_extend_from_M(partial);
        }
        Ok(result)
    }

    /// 统一加载入口：目录用 rvs_load_dir_BIS，文件用 rvs_parse。
    pub fn rvs_load_BIS(path: &Path) -> Result<Self, CapsMapError> {
        if path.is_dir() {
            Self::rvs_load_dir_BIS(path)
        } else if path.is_file() {
            Self::rvs_parse(
                &std::fs::read_to_string(path).map_err(|e| CapsMapError::FileRead {
                    path: path.display().to_string(),
                    error: e.to_string(),
                })?,
            )
        } else {
            Ok(Self::rvs_new())
        }
    }
}

/// 按 LAYER_ORDER 对文件路径排序。
/// 在 LAYER_ORDER 中的文件按层级顺序排，不在的按字母序排在后面。
fn rvs_sort_by_layer_M(files: &mut Vec<std::path::PathBuf>) {
    files.sort_by(|a, b| {
        let a_name = a
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let b_name = b
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let a_layer = LAYER_ORDER.iter().position(|&n| n == a_name);
        let b_layer = LAYER_ORDER.iter().position(|&n| n == b_name);
        match (a_layer, b_layer) {
            (Some(al), Some(bl)) => al.cmp(&bl),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_name.cmp(&b_name),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    #[test]
    fn test_20260425_capsmap_new_empty() {
        let cm = CapsMap::rvs_new();
        assert!(cm.rvs_lookup("anything").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_single() {
        let cm = CapsMap::rvs_parse("std::fs::read=BI").unwrap();
        let caps = cm.rvs_lookup("std::fs::read").unwrap();
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert_eq!(caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260425_capsmap_parse_empty_value() {
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        let caps = cm.rvs_lookup("HashMap::new").unwrap();
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260425_capsmap_parse_comments() {
        let cm = CapsMap::rvs_parse("# comment\nfunc=BI # inline\n").unwrap();
        assert!(cm.rvs_lookup("func").is_some());
        assert!(cm.rvs_lookup("# comment").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_missing_separator() {
        let result = CapsMap::rvs_parse("no_separator");
        assert!(result.is_err());
    }

    #[test]
    fn test_20260615_capsmap_parse_hash_in_key() {
        let cm = CapsMap::rvs_parse("exr::image::closure#0::crop_samples=S # SideEffect").unwrap();
        let caps = cm
            .rvs_lookup("exr::image::closure#0::crop_samples")
            .unwrap();
        assert!(caps.rvs_contains(Capability::S));
    }

    #[test]
    fn test_20260425_capsmap_parse_invalid_caps() {
        let result = CapsMap::rvs_parse("func=XYZ");
        assert!(result.is_err());
    }

    #[test]
    fn test_20260425_capsmap_lookup_exact_only() {
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        assert!(cm.rvs_lookup("HashMap::new").is_some());
        assert!(cm.rvs_lookup("HashMap").is_none());
    }

    #[test]
    fn test_20260425_capsmap_lookup_no_match() {
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        assert!(cm.rvs_lookup("nonexistent").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_empty_content() {
        let cm = CapsMap::rvs_parse("").unwrap();
        assert!(cm.rvs_lookup("anything").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_all_caps() {
        let cm = CapsMap::rvs_parse("danger=ABIMSTU").unwrap();
        let caps = cm.rvs_lookup("danger").unwrap();
        assert_eq!(caps.rvs_len(), 7);
    }

    #[test]
    fn test_20260611_seed_overrides_std() {
        let dir = std::env::temp_dir().join("test_20260611_seed_overrides_std");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), "func=S\nother_func=T\n").unwrap();
        std::fs::write(dir.join("std"), "func=U\nother_func=U\nnew_func=M\n").unwrap();
        let cm = CapsMap::rvs_load_dir_BIS(&dir).unwrap();
        let caps = cm.rvs_lookup("func").unwrap();
        assert!(caps.rvs_contains(Capability::S));
        assert!(!caps.rvs_contains(Capability::U));
        let other = cm.rvs_lookup("other_func").unwrap();
        assert!(other.rvs_contains(Capability::T));
        assert!(!other.rvs_contains(Capability::U));
        let new_func = cm.rvs_lookup("new_func").unwrap();
        assert!(new_func.rvs_contains(Capability::M));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260615_load_dir_layers() {
        let dir = std::env::temp_dir().join("test_20260615_load_dir_layers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), "func_a=S\n").unwrap();
        std::fs::write(dir.join("suppress"), "func_b=\n").unwrap();
        std::fs::write(dir.join("std"), "func_c=M\n").unwrap();
        let cm = CapsMap::rvs_load_dir_layers_BIS(&dir, &["seed", "suppress"]).unwrap();
        assert!(cm.rvs_lookup("func_a").is_some());
        assert!(cm.rvs_lookup("func_b").is_some());
        assert!(cm.rvs_lookup("func_c").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260615_load_dir_excluding() {
        let dir = std::env::temp_dir().join("test_20260615_load_dir_excluding");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), "func_a=S\n").unwrap();
        std::fs::write(dir.join("deps"), "func_b=T\n").unwrap();
        std::fs::write(dir.join("ext"), "func_c=M\n").unwrap();
        let cm = CapsMap::rvs_load_dir_excluding_BIS(&dir, &["deps"]).unwrap();
        assert!(cm.rvs_lookup("func_a").is_some());
        assert!(cm.rvs_lookup("func_b").is_none());
        assert!(cm.rvs_lookup("func_c").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260615_load_single_file() {
        let path = std::env::temp_dir().join("test_20260615_load_single_file.txt");
        std::fs::write(&path, "func=BI\n").unwrap();
        let cm = CapsMap::rvs_load_BIS(&path).unwrap();
        assert!(cm.rvs_lookup("func").is_some());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_20260615_load_nonexistent() {
        let cm = CapsMap::rvs_load_BIS(std::path::Path::new("/nonexistent/path")).unwrap();
        assert!(cm.rvs_lookup("anything").is_none());
    }
}
