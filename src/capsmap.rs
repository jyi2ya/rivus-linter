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
    pub fn rvs_len(&self) -> usize {
        self.entries.len()
    }

    /// 构造一个空的能力映射表。
    pub fn rvs_new() -> Self {
        Self::default()
    }

    /// 册子一行一行翻，键值各归其位。
    /// 注释以井号起，等号为界，空行跳过。
    pub fn rvs_parse(content: &str) -> Result<Self, CapsMapError> {
        let mut entries = BTreeMap::new();
        for (i, raw_line) in content.lines().enumerate() {
            let line_num = i + 1;
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
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

    /// 按名索骥：先查全名，再查尾名。
    /// 全名若 `std::fs::read_to_string`，尾名即 `read_to_string`。
    /// 路径调用如 `Vec::new` 亦可匹配 `alloc::vec::Vec::new`。
    pub fn rvs_lookup(&self, name: &str) -> Option<&CapabilitySet> {
        if let Some(caps) = self.entries.get(name) {
            return Some(caps);
        }
        for (key, caps) in &self.entries {
            if name.ends_with(&format!("::{key}")) || key.ends_with(&format!("::{name}")) {
                return Some(caps);
            }
        }
        None
    }

    /// 合并两个能力映射表：self 的条目优先，other 的同名键被覆盖。
    pub fn rvs_merge(self, other: Self) -> Self {
        let mut entries = other.entries;
        for (key, caps) in self.entries {
            entries.insert(key, caps);
        }
        Self { entries }
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
    ///   1. seed   — manually maintained low-level overrides (lowest priority)
    ///   2. std    — auto-generated std/core/alloc caps (overrides seed)
    ///   3. deps   — auto-generated external dependency caps (overrides std)
    ///   4. ext    — manually maintained project-specific caps (highest priority)
    ///
    /// Files not named seed/std/deps/ext are loaded alphabetically after ext.
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
        // Sort: seed first, then std, deps, ext, then alphabetical for others.
        // Files loaded later override earlier ones (rvs_extend_from_M).
        const LAYER_ORDER: &[&str] = &["seed", "std", "deps", "ext"];
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
    fn test_20260425_capsmap_parse_invalid_caps() {
        let result = CapsMap::rvs_parse("func=XYZ");
        assert!(result.is_err());
    }

    #[test]
    fn test_20260425_capsmap_lookup_suffix_match() {
        let cm = CapsMap::rvs_parse("HashMap::new=").unwrap();
        let caps = cm.rvs_lookup("std::collections::HashMap::new");
        assert!(caps.is_some());
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
}
