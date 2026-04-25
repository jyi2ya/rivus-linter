use std::collections::BTreeMap;

use crate::capability::CapabilitySet;

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
    },
    #[error("line {line}: missing '=' separator")]
    MissingSeparator { line: usize },
}

impl CapsMap {
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
                CapabilitySet::rvs_from_str(value).map_err(|_| CapsMapError::InvalidCaps {
                    key: key.clone(),
                    caps: value.to_string(),
                    line: line_num,
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
