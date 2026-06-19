use toml_edit::{DocumentMut, Item, Table};

pub const CLIPPY_LINTS: &[(&str, &str)] = &[
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
    ("let_underscore_future", "warn"),
    ("let_underscore_must_use", "warn"),
    ("unused_result_ok", "warn"),
    ("map_err_ignore", "warn"),
    ("assertions_on_result_states", "warn"),
    ("await_holding_lock", "warn"),
    ("await_holding_refcell_ref", "warn"),
    ("large_futures", "warn"),
    ("mem_forget", "warn"),
    ("undocumented_unsafe_blocks", "warn"),
    ("multiple_unsafe_ops_per_block", "warn"),
    ("unnecessary_safety_doc", "warn"),
    ("unnecessary_safety_comment", "warn"),
    ("float_cmp", "warn"),
    ("float_cmp_const", "warn"),
    ("lossy_float_literal", "warn"),
    ("cast_sign_loss", "warn"),
    ("invalid_upcast_comparisons", "warn"),
    ("rc_mutex", "warn"),
    ("debug_assert_with_mut_call", "warn"),
    ("iter_not_returning_iterator", "warn"),
    ("expl_impl_clone_on_copy", "warn"),
    ("infallible_try_from", "warn"),
    ("use_debug", "warn"),
    ("dbg_macro", "warn"),
    ("allow_attributes", "warn"),
    ("allow_attributes_without_reason", "warn"),
    ("manual_ok_err", "allow"),
];

/// Inject clippy lint rules into a Cargo.toml string.
/// Returns the new Cargo.toml string and the count of injected lints.
pub fn rvs_inject_clippy_lints_M(cargo_toml: &str) -> (String, usize) {
    let mut doc: DocumentMut = match cargo_toml.parse() {
        Ok(d) => d,
        Err(_) => return (cargo_toml.to_string(), 0),
    };

    let lints = doc.entry("lints").or_insert(Item::Table(Table::new()));
    let clippy = lints.as_table_mut().and_then(|t| {
        t.entry("clippy")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
    });

    let Some(clippy_table) = clippy else {
        return (cargo_toml.to_string(), 0);
    };

    let mut count = 0;
    for (name, level) in CLIPPY_LINTS {
        if !clippy_table.contains_key(name) {
            clippy_table.insert(name, toml_edit::value(*level));
            count += 1;
        }
    }

    if count == 0 {
        return (cargo_toml.to_string(), 0);
    }

    (doc.to_string(), count)
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
}
