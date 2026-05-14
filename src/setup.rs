//! Setup command: inject clippy lint rules into Cargo.toml and copy rivus.md to AGENTS.md.

use std::collections::HashSet;

/// Clippy lint rules to inject into Cargo.toml [lints.clippy].
/// Each entry is (lint_name, level).
pub const CLIPPY_LINTS: &[(&str, &str)] = &[
    // Don't panic
    ("string_slice", "warn"),
    ("indexing_slicing", "warn"),
    ("unwrap_used", "warn"),
    ("panic", "warn"),
    ("todo", "warn"),
    ("unimplemented", "warn"),
    ("unreachable", "warn"),
    ("get_unwrap", "warn"),
    ("unwrap_in_result", "warn"),
    ("unchecked_time_subtraction", "warn"),
    ("panic_in_result_fn", "warn"),
    // Don't fail silently
    ("let_underscore_future", "warn"),
    ("let_underscore_must_use", "warn"),
    ("unused_result_ok", "warn"),
    ("map_err_ignore", "warn"),
    ("assertions_on_result_states", "warn"),
    // Don't do bad async
    ("await_holding_lock", "warn"),
    ("await_holding_refcell_ref", "warn"),
    ("large_futures", "warn"),
    // Don't be unsafe with memory
    ("mem_forget", "warn"),
    ("undocumented_unsafe_blocks", "warn"),
    ("multiple_unsafe_ops_per_block", "warn"),
    ("unnecessary_safety_doc", "warn"),
    ("unnecessary_safety_comment", "warn"),
    // Don't be potentially wrong about numbers
    ("float_cmp", "warn"),
    ("float_cmp_const", "warn"),
    ("lossy_float_literal", "warn"),
    ("cast_sign_loss", "warn"),
    ("invalid_upcast_comparisons", "warn"),
    // Miscellaneous
    ("rc_mutex", "warn"),
    ("debug_assert_with_mut_call", "warn"),
    ("iter_not_returning_iterator", "warn"),
    ("expl_impl_clone_on_copy", "warn"),
    ("infallible_try_from", "warn"),
    ("use_debug", "warn"),
    ("dbg_macro", "warn"),
    ("allow_attributes", "warn"),
    ("allow_attributes_without_reason", "warn"),
];

/// Spawn 函数的 capsmap 条目：函数路径及其能力。
/// Setup 时注入到目标项目的 capsmap.txt 中。
pub const SPAWN_CAPSMAP_ENTRIES: &[(&str, &str)] = &[
    ("tokio::spawn", "AS"),
    ("tokio::task::spawn", "AS"),
    ("tokio::task::spawn_blocking", "BIS"),
    ("tokio::task::spawn_local", "AST"),
    ("std::thread::spawn", "BS"),
    ("std::thread::Builder::spawn", "BUS"),
    ("async_std::task::spawn", "AS"),
    ("async_std::task::spawn_blocking", "BIS"),
    ("smol::spawn", "AS"),
];

/// 检查 capsmap.txt 内容中是否包含 spawn 函数条目，将缺失的追加到末尾。
/// 返回 `(new_content, count_of_injected)`。
pub fn rvs_inject_spawn_capsmap_M(capsmap: &str) -> (String, usize) {
    let mut existing_keys: HashSet<&str> = HashSet::new();
    for line in capsmap.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            existing_keys.insert(key.trim());
        }
    }

    let missing: Vec<(&str, &str)> = SPAWN_CAPSMAP_ENTRIES
        .iter()
        .filter(|(key, _)| !existing_keys.contains(*key))
        .copied()
        .collect();

    if missing.is_empty() {
        return (capsmap.to_string(), 0);
    }

    let mut result = capsmap.to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str("\n# ─── 非结构化 spawn 函数（并发 goto）─────────────────────────\n");
    result.push_str("# spawn 创建的后台任务不受调用方作用域约束，容易导致资源泄漏、错误丢失。\n");
    result
        .push_str("# 应改用 join!、JoinSet、FuturesUnordered、thread::scope 等结构化并发原语。\n");
    for (key, caps) in &missing {
        result.push_str(&format!("{key}={caps}\n"));
    }

    (result, missing.len())
}

/// Injects missing clippy lint entries under `[lints.clippy]` in a Cargo.toml string.
/// Returns `(new_content, count_of_injected)`.
pub fn rvs_inject_clippy_lints_M(cargo_toml: &str) -> (String, usize) {
    let mut existing: HashSet<&str> = HashSet::new();
    let mut in_lints_clippy = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[lints.clippy]" {
            in_lints_clippy = true;
            continue;
        }
        if in_lints_clippy {
            if trimmed.starts_with('[') {
                in_lints_clippy = false;
            } else if let Some((key, _val)) = trimmed.split_once('=') {
                existing.insert(key.trim());
            }
        }
    }

    let missing: Vec<(&str, &str)> = CLIPPY_LINTS
        .iter()
        .filter(|(name, _)| !existing.contains(*name))
        .copied()
        .collect();

    if missing.is_empty() {
        return (cargo_toml.to_string(), 0);
    }

    let mut result = cargo_toml.to_string();

    if existing.is_empty() {
        // No [lints.clippy] section exists; append one
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("\n[lints.clippy]\n");
        for (name, level) in &missing {
            result.push_str(&format!("{name} = \"{level}\"\n"));
        }
    } else {
        // Section exists; find it and append missing entries at the end of the section
        let mut lines: Vec<String> = result.lines().map(String::from).collect();
        let mut section_end = lines.len();

        let mut found = false;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed == "[lints.clippy]" {
                found = true;
                continue;
            }
            if found && trimmed.starts_with('[') {
                section_end = i;
                break;
            }
        }

        let mut insert_at = section_end;
        // Walk backwards to skip trailing blank lines in the section
        while insert_at > 0
            && lines
                .get(insert_at - 1)
                .is_some_and(|l| l.trim().is_empty())
        {
            insert_at -= 1;
        }

        for (offset, entry) in missing
            .iter()
            .map(|(name, level)| format!("{name} = \"{level}\""))
            .enumerate()
        {
            lines.insert(insert_at + offset, entry);
        }

        result = lines.join("\n");
        result.push('\n');
    }

    (result, missing.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260501_inject_into_empty_cargo_toml() {
        let input = "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        debug_assert_eq!(count, CLIPPY_LINTS.len());
        debug_assert!(result.contains("[lints.clippy]"));
        debug_assert!(result.contains("string_slice = \"warn\""));
        debug_assert!(result.contains("allow_attributes_without_reason = \"warn\""));

        let expected = format!("{input}\n[lints.clippy]\n");
        debug_assert!(
            result.starts_with(&expected),
            "result should start with original + section header"
        );
    }

    #[test]
    fn test_20260502_inject_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, count1) = rvs_inject_clippy_lints_M(input);
        let (second, count2) = rvs_inject_clippy_lints_M(&first);
        debug_assert!(count1 > 0);
        debug_assert_eq!(count2, 0);
        debug_assert_eq!(first, second);
    }

    #[test]
    fn test_20260503_inject_preserves_existing() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\nunwrap_used = \"warn\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        debug_assert!(result.contains("string_slice = \"deny\""));
        debug_assert!(result.contains("unwrap_used = \"warn\""));
        debug_assert_eq!(count, CLIPPY_LINTS.len() - 2);
    }

    #[test]
    fn test_20260504_inject_no_section_in_middle() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\nserde = \"1\"\n\n[features]\ndefault = []\n";
        let (result, count) = rvs_inject_clippy_lints_M(input);
        debug_assert!(count > 0);
        debug_assert!(result.contains("[lints.clippy]"));
        let features_pos = result.find("[features]").unwrap();
        let lints_pos = result.find("[lints.clippy]").unwrap();
        debug_assert!(lints_pos > features_pos);
    }

    #[test]
    fn test_20260515_inject_spawn_capsmap_empty() {
        let input = "HashMap::new=\nVec::push=\n";
        let (result, count) = rvs_inject_spawn_capsmap_M(input);
        debug_assert_eq!(count, SPAWN_CAPSMAP_ENTRIES.len());
        debug_assert!(result.contains("tokio::spawn=AS"));
        debug_assert!(result.contains("std::thread::spawn=BS"));
    }

    #[test]
    fn test_20260515_inject_spawn_capsmap_idempotent() {
        let input = "HashMap::new=\n";
        let (first, count1) = rvs_inject_spawn_capsmap_M(input);
        let (second, count2) = rvs_inject_spawn_capsmap_M(&first);
        debug_assert!(count1 > 0);
        debug_assert_eq!(count2, 0);
        debug_assert_eq!(first, second);
    }

    #[test]
    fn test_20260515_inject_spawn_capsmap_partial() {
        let input = "HashMap::new=\ntokio::spawn=AS\nstd::thread::spawn=BS\n";
        let (result, count) = rvs_inject_spawn_capsmap_M(input);
        debug_assert!(count > 0);
        debug_assert!(count < SPAWN_CAPSMAP_ENTRIES.len());
        debug_assert!(result.contains("tokio::task::spawn=AS"));
    }
}
