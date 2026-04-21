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
            let line = raw_line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or(CapsMapError::MissingSeparator { line: line_num })?;
            let key = key.trim().to_string();
            let value = value.split('#').next().unwrap_or_default().trim();
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
