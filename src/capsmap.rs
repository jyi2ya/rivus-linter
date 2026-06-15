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

impl CapsMap {
    /// 构造一个空的能力映射表。
    pub fn rvs_new() -> Self {
        Self::default()
    }

    /// 册子一行一行翻，键值各归其位。
    /// 注释以井号起，但井号在键中合法（如 `closure#0`），
    /// 因此只取等号之后的值部分中的注释。
    /// 行首 `#` 注释整行。
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
            // Only strip comments from the value part, not the key.
            // Keys may contain '#' (e.g. closure#0 in def_path).
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

    /// 按名索骥：精确匹配，不做后缀匹配。
    ///
    /// caps 文件中的键必须使用 rustc 给出的 def_path（全限定路径），
    /// 如 `std::io::Read::read` 或 `core::option::unwrap`。
    /// 短名只在警告输出时作为辅助信息显示给人类，不参与查找。
    pub fn rvs_lookup(&self, name: &str) -> Option<&CapabilitySet> {
        self.entries.get(name)
    }

    /// Extend this capsmap with entries from another.
    /// Entries in `other` override existing entries for the same key.
    pub(crate) fn rvs_extend_from_M(&mut self, other: Self) {
        for (key, caps) in other.entries {
            self.entries.insert(key, caps);
        }
    }

    /// Load all capability files from a directory, merging them in layer order.
    ///
    /// Layer priority (highest last, overrides earlier):
    ///   1. std      — auto-generated std/core/alloc caps (lowest priority)
    ///   2. deps     — auto-generated external dependency caps (overrides std)
    ///   3. seed     — manually maintained low-level overrides (overrides auto-generated)
    ///   4. suppress — corrections for std/core/alloc functions whose inferred caps are too broad
    ///   5. ext      — manually maintained project-specific caps (highest priority)
    ///
    /// Files not named std/deps/seed/suppress/ext are loaded alphabetically after ext.
    pub fn rvs_load_from_dir_BIMS(dir: &Path) -> Result<Self, CapsMapError> {
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
        // Sort: std first, then deps, seed, ext, then alphabetical for others.
        // Files loaded later override earlier ones (rvs_extend_from_M).
        const LAYER_ORDER: &[&str] = &["std", "deps", "seed", "suppress", "ext"];
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
        let content = "# comment\nstd::fs::read=BI # inline\n\nstd::process::exit=S\n";
        let cm = CapsMap::rvs_parse(content).unwrap();
        assert!(cm.rvs_lookup("std::fs::read").is_some());
        assert!(cm.rvs_lookup("std::process::exit").is_some());
    }

    #[test]
    fn test_20260425_capsmap_parse_missing_separator() {
        let result = CapsMap::rvs_parse("no_equals");
        assert!(result.is_err());
    }

    #[test]
    fn test_20260615_capsmap_parse_hash_in_key() {
        // Def paths may contain '#' (e.g. closure#0 in rustc def_path).
        // The '#' in the key must not be treated as a comment marker.
        let cm = CapsMap::rvs_parse("exr::image::closure#0::crop_samples=P # Panic").unwrap();
        let caps = cm
            .rvs_lookup("exr::image::closure#0::crop_samples")
            .unwrap();
        assert!(caps.rvs_contains(Capability::P));
    }

    #[test]
    fn test_20260425_capsmap_parse_invalid_caps() {
        let result = CapsMap::rvs_parse("func=XYZ");
        assert!(result.is_err());
    }

    #[test]
    fn test_20260425_capsmap_lookup_exact_only() {
        // Suffix matching has been removed — all lookups are exact.
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        // Exact match works
        assert!(cm.rvs_lookup("HashMap::new").is_some());
        // Suffix match does NOT work anymore
        assert!(cm.rvs_lookup("std::collections::HashMap::new").is_none());
    }

    #[test]
    fn test_20260425_capsmap_lookup_no_match() {
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        assert!(cm.rvs_lookup("HashMap::insert").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_empty_content() {
        let cm = CapsMap::rvs_parse("").unwrap();
        assert!(cm.rvs_lookup("anything").is_none());
    }

    #[test]
    fn test_20260425_capsmap_parse_all_caps() {
        let cm = CapsMap::rvs_parse("danger=ABIMPSTU").unwrap();
        let caps = cm.rvs_lookup("danger").unwrap();
        assert_eq!(caps.rvs_len(), 8);
    }

    #[test]
    fn test_20260611_seed_overrides_std() {
        let dir = std::env::temp_dir().join("test_20260611_seed_overrides_std");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), "func=S\nother_func=P\n").unwrap();
        std::fs::write(dir.join("std"), "func=U\nother_func=U\nnew_func=M\n").unwrap();
        let cm = CapsMap::rvs_load_from_dir_BIMS(&dir).unwrap();
        let caps = cm.rvs_lookup("func").unwrap();
        assert!(caps.rvs_contains(Capability::S));
        assert!(!caps.rvs_contains(Capability::U));
        let other = cm.rvs_lookup("other_func").unwrap();
        assert!(other.rvs_contains(Capability::P));
        assert!(!other.rvs_contains(Capability::U));
        let new_func = cm.rvs_lookup("new_func").unwrap();
        assert!(new_func.rvs_contains(Capability::M));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
