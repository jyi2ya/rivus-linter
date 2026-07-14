use std::path::Path;

use toml_edit::{DocumentMut, Item, Table};

use crate::fs_guard::rvs_render_atomic_write_failure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupFileRequirement {
    MustExist,
    Optional,
}

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
    ("manual_unwrap_or_default", "allow"),
];

/// Inject clippy lint rules into a Cargo.toml string.
/// Returns the new Cargo.toml string and the count of injected lints.
#[cfg(test)]
pub fn rvs_inject_clippy_lints_M(cargo_toml: &str) -> Result<(String, usize), String> {
    let mut doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|e| format!("invalid TOML: {e}"))?;

    let count = rvs_inject_clippy_lints_into_document_M(&mut doc)?;
    if count == 0 {
        return Ok((cargo_toml.to_string(), 0));
    }

    Ok((doc.to_string(), count))
}

fn rvs_inject_clippy_lints_into_document_M(doc: &mut DocumentMut) -> Result<usize, String> {
    let lints = doc.entry("lints").or_insert(Item::Table(Table::new()));
    let Some(lints_table) = lints.as_table_mut() else {
        return Err("[lints] must be a table".into());
    };
    let clippy = lints_table
        .entry("clippy")
        .or_insert(Item::Table(Table::new()));

    let Some(clippy_table) = clippy.as_table_mut() else {
        return Err("[lints.clippy] must be a table".into());
    };

    let mut count = 0;
    for (name, level) in CLIPPY_LINTS {
        if !clippy_table.contains_key(name) {
            clippy_table.insert(name, toml_edit::value(*level));
            count += 1;
        }
    }
    Ok(count)
}

pub(crate) fn rvs_run_setup_BIMS(path: &Path) -> Result<(), String> {
    let cargo_toml_path = path.join("Cargo.toml");
    let agents_md = path.join("AGENTS.md");
    rvs_preflight_setup_file_BIS(
        &cargo_toml_path,
        "Cargo.toml",
        &SetupFileRequirement::MustExist,
    )?;
    rvs_preflight_setup_file_BIS(&agents_md, "AGENTS.md", &SetupFileRequirement::Optional)?;
    crate::workspace::rvs_ensure_cargo_project_BIS(path)?;

    let project = crate::cargo_targets::rvs_load_cargo_project_model_BIS(path)?;
    crate::cargo_targets::rvs_collect_local_crate_prefixes_from_model_BIS(
        path,
        &project,
        crate::cargo_targets::CargoTargetScope::WithTestExampleBench,
    )?;
    let (content, mut document) = project.rvs_into_source_and_document();

    let count = rvs_inject_clippy_lints_into_document_M(&mut document).map_err(|e| {
        format!(
            "cannot inject clippy lints into '{}': {e}",
            cargo_toml_path.display()
        )
    })?;
    let new_content = if count > 0 {
        document.to_string()
    } else {
        content.clone()
    };

    if count > 0 {
        rvs_write_file_atomic_BIS(&cargo_toml_path, &new_content)?;
        println!(
            "Injected {count} clippy lint(s) into {}",
            cargo_toml_path.display()
        );
    } else {
        println!(
            "All clippy lints already present in {}",
            cargo_toml_path.display()
        );
    }
    if let Err(e) = rvs_write_file_atomic_BIS(&agents_md, crate::RIVUS_MD) {
        if count > 0
            && let Err(restore_error) = rvs_write_file_atomic_BIS(&cargo_toml_path, &content)
        {
            return Err(format!(
                "cannot write '{}': {e}; additionally failed to restore '{}': {restore_error}",
                agents_md.display(),
                cargo_toml_path.display()
            ));
        }
        return Err(format!("cannot write '{}': {e}", agents_md.display()));
    }
    println!("Written {}", agents_md.display());

    Ok(())
}

fn rvs_preflight_setup_file_BIS(
    path: &Path,
    label: &str,
    requirement: &SetupFileRequirement,
) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{label} must not be a symlink: {}", path.display()))
        }
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "{label} must be a regular file: {}",
            path.display()
        )),
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && *requirement == SetupFileRequirement::Optional =>
        {
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{label} not found: {}", path.display()))
        }
        Err(e) => Err(format!("cannot inspect {}: {e}", path.display())),
    }
}

fn rvs_write_file_atomic_BIS(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("cannot determine parent for '{}'", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("cannot determine file name for '{}'", path.display()))?
        .to_string_lossy();
    let temp_path_for_attempt = |attempt| {
        parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            attempt
        ))
    };
    crate::fs_guard::rvs_write_atomic_BIS(path, content.as_bytes(), &temp_path_for_attempt)
        .map_err(|failure| rvs_render_atomic_write_failure(failure, path, "temp file", true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    #[test]
    fn test_20260501_inject_into_empty_cargo_toml() {
        let input = "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input).unwrap();
        debug_assert_eq!(count, CLIPPY_LINTS.len());
        debug_assert!(result.contains("[lints.clippy]"));
        debug_assert!(result.contains("string_slice = \"warn\""));
        debug_assert!(result.contains("allow_attributes_without_reason = \"warn\""));
    }

    #[test]
    fn test_20260502_inject_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, count1) = rvs_inject_clippy_lints_M(input).unwrap();
        let (second, count2) = rvs_inject_clippy_lints_M(&first).unwrap();
        debug_assert!(count1 > 0);
        debug_assert_eq!(count2, 0);
        debug_assert_eq!(first, second);
    }

    #[test]
    fn test_20260503_inject_preserves_existing() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\nunwrap_used = \"warn\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input).unwrap();
        debug_assert!(result.contains("string_slice = \"deny\""));
        debug_assert!(result.contains("unwrap_used = \"warn\""));
        debug_assert_eq!(count, CLIPPY_LINTS.len() - 2);
    }

    #[test]
    fn test_20260607_setup_inject_clippy_empty() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input).unwrap();
        rvs_snapshot_BIS(
            "test_20260607_setup_inject_clippy_empty",
            &format!("count: {count}\n{result}"),
        );
        assert_eq!(count, CLIPPY_LINTS.len());
        assert!(result.contains("[lints.clippy]"));
    }

    #[test]
    fn test_20260607_setup_inject_clippy_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, c1) = rvs_inject_clippy_lints_M(input).unwrap();
        let (second, c2) = rvs_inject_clippy_lints_M(&first).unwrap();
        assert!(c1 > 0);
        assert_eq!(c2, 0);
        assert_eq!(first, second);
    }

    #[test]
    fn test_20260607_setup_inject_clippy_preserves() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints_M(input).unwrap();
        assert!(result.contains("string_slice = \"deny\""));
        assert_eq!(count, CLIPPY_LINTS.len() - 1);
    }

    #[test]
    fn test_20260702_setup_rejects_non_cargo_dir_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-non-cargo");
        let agents_md = dir.join("AGENTS.md");
        let result = rvs_run_setup_BIMS(&dir);
        let output = format!("result={result:?}\nexists={}\n", agents_md.exists())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_setup_rejects_non_cargo_dir_without_writing_agents",
            &output,
        );

        assert!(result.is_err(), "setup should fail for non-cargo dir");
        assert!(
            !agents_md.exists(),
            "setup should not write AGENTS.md on failure"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_setup_rejects_invalid_cargo_toml_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-invalid-cargo-toml");
        std::fs::write(dir.join("Cargo.toml"), "[package\nname = \"broken\"\n").unwrap();
        let agents_md = dir.join("AGENTS.md");

        let result = rvs_run_setup_BIMS(&dir);
        let output = format!("result={result:?}\nexists={}\n", agents_md.exists())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_setup_rejects_invalid_cargo_toml_without_writing_agents",
            &output,
        );

        assert!(result.is_err(), "setup should fail for invalid Cargo.toml");
        assert!(
            !agents_md.exists(),
            "setup should not write AGENTS.md when Cargo.toml is invalid"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_setup_rejects_non_table_lints_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-non-table-lints");
        std::fs::write(
            dir.join("Cargo.toml"),
            "lints = \"bad\"\n\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let agents_md = dir.join("AGENTS.md");

        let result = rvs_run_setup_BIMS(&dir);
        let output = format!("result={result:?}\nexists={}\n", agents_md.exists())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_setup_rejects_non_table_lints_without_writing_agents",
            &output,
        );

        assert!(result.is_err(), "setup should fail for non-table lints");
        assert!(
            !agents_md.exists(),
            "setup should not write AGENTS.md on failure"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_setup_rejects_non_table_clippy_lints_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-non-table-clippy-lints");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lints]\nclippy = \"bad\"\n",
        )
        .unwrap();
        let agents_md = dir.join("AGENTS.md");

        let result = rvs_run_setup_BIMS(&dir);
        let output = format!("result={result:?}\nexists={}\n", agents_md.exists())
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_setup_rejects_non_table_clippy_lints_without_writing_agents",
            &output,
        );

        assert!(
            result.is_err(),
            "setup should fail for non-table lints.clippy"
        );
        assert!(
            !agents_md.exists(),
            "setup should not write AGENTS.md on failure"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_setup_rejects_agents_directory_without_writing_cargo() {
        let dir = rvs_make_temp_dir_BIS("setup-agents-directory");
        let cargo_toml = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("AGENTS.md")).unwrap();

        let result = rvs_run_setup_BIMS(&dir);
        let restored = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let output = format!(
            "result={result:?}\nrestored={}\nagents_is_dir={}\n",
            restored == cargo_toml,
            dir.join("AGENTS.md").is_dir()
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260707_setup_rejects_agents_directory_without_writing_cargo",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(restored, cargo_toml);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_setup_rejects_agents_symlink_without_writing_cargo() {
        let dir = rvs_make_temp_dir_BIS("setup-agents-symlink");
        let cargo_toml = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        let victim = dir.join("victim.md");
        std::fs::write(&victim, "victim\n").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("AGENTS.md")).unwrap();

        let result = rvs_run_setup_BIMS(&dir);
        let cargo_after = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let victim_after = std::fs::read_to_string(&victim).unwrap();
        let output = format!(
            "result={result:?}\ncargo_unchanged={}\nvictim_unchanged={}\n",
            cargo_after == cargo_toml,
            victim_after == "victim\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_setup_rejects_agents_symlink_without_writing_cargo",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(cargo_after, cargo_toml);
        assert_eq!(victim_after, "victim\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_setup_rejects_cargo_toml_symlink_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-cargo-symlink");
        let cargo_toml = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        let external = dir.join("external-Cargo.toml");
        std::fs::write(&external, cargo_toml).unwrap();
        std::os::unix::fs::symlink(&external, dir.join("Cargo.toml")).unwrap();

        let result = rvs_run_setup_BIMS(&dir);
        let external_after = std::fs::read_to_string(&external).unwrap();
        let agents_exists = dir.join("AGENTS.md").exists();
        let output = format!(
            "result={result:?}\nexternal_unchanged={}\nagents_exists={agents_exists}\n",
            external_after == cargo_toml
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_setup_rejects_cargo_toml_symlink_without_writing_agents",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(external_after, cargo_toml);
        assert!(!agents_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
