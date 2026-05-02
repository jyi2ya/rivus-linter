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
    ("dbg_macro", "warn"),
    ("allow_attributes", "warn"),
    ("allow_attributes_without_reason", "warn"),
];

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
        // Should be appended after [features], not in the middle
        let features_pos = result.find("[features]").unwrap();
        let lints_pos = result.find("[lints.clippy]").unwrap();
        debug_assert!(lints_pos > features_pos);
    }
}
