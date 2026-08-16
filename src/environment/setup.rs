use std::path::{Path, PathBuf};

use snafu::Snafu;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

pub(crate) const RIVUS_AGENTS_BEGIN_MARKER: &str =
    "<!-- BEGIN RIVUS MANAGED SECTION: cargo-rivus setup -->";
pub(crate) const RIVUS_AGENTS_END_MARKER: &str =
    "<!-- END RIVUS MANAGED SECTION: cargo-rivus setup -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupFileRequirement {
    MustExist,
    Optional,
}

#[derive(Debug, Snafu)]
pub(crate) enum ClippyConfigError {
    #[cfg(test)]
    #[snafu(display("invalid TOML: {source}"))]
    InvalidToml { source: toml_edit::TomlError },
    #[snafu(display("[lints] must be a table or inline table"))]
    LintsMustBeTable,
    #[snafu(display("[lints.clippy] must be a table or inline table"))]
    ClippyMustBeTable,
    #[snafu(display(
        "[lints] inherits workspace lints; add the Rivus entries to [workspace.lints.clippy] in the workspace root, or stop inheriting before rerunning setup"
    ))]
    WorkspaceLintsOwned,
}

#[derive(Debug, Snafu)]
pub(crate) enum ManagedSectionError {
    #[snafu(display(
        "managed marker conflict: {reason}; keep exactly one '{begin}' line followed by exactly one '{end}' line, or remove every Rivus marker before rerunning setup"
    ))]
    MarkerConflict {
        reason: String,
        begin: &'static str,
        end: &'static str,
    },
}

#[derive(Debug, Snafu)]
pub(crate) enum SetupError {
    #[snafu(display("{label} must not be a symlink: {}", path.display()))]
    SymlinkTarget { label: &'static str, path: PathBuf },
    #[snafu(display("{label} must be a regular file: {}", path.display()))]
    NonRegularTarget { label: &'static str, path: PathBuf },
    #[snafu(display("{label} not found: {}", path.display()))]
    MissingTarget { label: &'static str, path: PathBuf },
    #[snafu(display("cannot inspect '{}': {source}", path.display()))]
    InspectTarget {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("cannot read '{}': {source}", path.display()))]
    ReadTarget {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("{message}"))]
    ProjectValidation { message: String },
    #[snafu(display("cannot merge clippy lints into '{}': {source}", path.display()))]
    ClippyConfig {
        path: PathBuf,
        source: ClippyConfigError,
    },
    #[snafu(display("cannot update managed section in '{}': {source}", path.display()))]
    ManagedDocument {
        path: PathBuf,
        source: ManagedSectionError,
    },
    #[snafu(display("cannot write '{}': {source}", path.display()))]
    WriteTarget {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "{write_error}; additionally failed to restore '{}': {rollback_error}",
        rollback_path.display()
    ))]
    WriteAndRollback {
        write_error: String,
        rollback_path: PathBuf,
        rollback_error: String,
    },
}

fn rvs_clippy_lints() -> Vec<(String, String)> {
    let document: DocumentMut = include_str!("../../Cargo.toml")
        .parse()
        .expect("never: rivus-linter Cargo.toml must be valid TOML");
    document
        .get("lints")
        .and_then(Item::as_table)
        .and_then(|lints| lints.get("clippy"))
        .and_then(Item::as_table)
        .expect("never: rivus-linter Cargo.toml defines [lints.clippy]")
        .iter()
        .map(|(name, level)| {
            (
                name.to_string(),
                level
                    .as_str()
                    .expect("never: clippy lint levels are strings")
                    .to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn rvs_inject_clippy_lints(
    cargo_toml: &str,
) -> Result<(String, usize), ClippyConfigError> {
    let mut doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|source| ClippyConfigError::InvalidToml { source })?;

    let count = rvs_inject_clippy_lints_into_document_M(&mut doc)?;
    if count == 0 {
        return Ok((cargo_toml.to_string(), 0));
    }

    Ok((doc.to_string(), count))
}

fn rvs_inject_clippy_lints_into_document_M(
    doc: &mut DocumentMut,
) -> Result<usize, ClippyConfigError> {
    let lints = doc.entry("lints").or_insert(Item::Table(Table::new()));
    let lints_is_inline = lints.is_inline_table();
    let Some(lints_table) = lints.as_table_like_mut() else {
        return Err(ClippyConfigError::LintsMustBeTable);
    };
    if lints_table.contains_key("workspace") {
        return Err(ClippyConfigError::WorkspaceLintsOwned);
    }
    if !lints_table.contains_key("clippy") {
        let clippy = if lints_is_inline {
            Item::Value(Value::InlineTable(InlineTable::new()))
        } else {
            Item::Table(Table::new())
        };
        let previous = lints_table.insert("clippy", clippy);
        debug_assert!(
            previous.is_none(),
            "clippy entry was absent before insertion"
        );
    }

    let clippy = lints_table
        .get_mut("clippy")
        .expect("never: clippy table was present or inserted");
    let Some(clippy_table) = clippy.as_table_like_mut() else {
        return Err(ClippyConfigError::ClippyMustBeTable);
    };

    let mut count = 0;
    for (name, level) in rvs_clippy_lints() {
        if !clippy_table.contains_key(&name) {
            clippy_table.insert(&name, toml_edit::value(level));
            count += 1;
        }
    }
    Ok(count)
}

fn rvs_marker_positions(
    content: &str,
    marker: &'static str,
) -> Result<Vec<usize>, ManagedSectionError> {
    debug_assert!(!marker.is_empty(), "managed marker is nonempty");
    let mut positions = Vec::new();
    for (position, _) in content.match_indices(marker) {
        let starts_line = position == 0
            || content
                .as_bytes()
                .get(position.saturating_sub(1))
                .is_some_and(|byte| *byte == b'\n');
        let suffix = content
            .get(position + marker.len()..)
            .expect("never: match position ends on a UTF-8 boundary");
        let ends_line = suffix.is_empty() || suffix.starts_with('\n') || suffix.starts_with("\r\n");
        if !starts_line || !ends_line {
            return Err(ManagedSectionError::MarkerConflict {
                reason: format!("'{marker}' must occupy a dedicated line"),
                begin: RIVUS_AGENTS_BEGIN_MARKER,
                end: RIVUS_AGENTS_END_MARKER,
            });
        }
        positions.push(position);
    }
    Ok(positions)
}

fn rvs_managed_agents_block() -> String {
    debug_assert!(!crate::RIVUS_PROJECT_TEMPLATE.contains(RIVUS_AGENTS_BEGIN_MARKER));
    debug_assert!(!crate::RIVUS_PROJECT_TEMPLATE.contains(RIVUS_AGENTS_END_MARKER));
    format!(
        "{RIVUS_AGENTS_BEGIN_MARKER}\n{}{RIVUS_AGENTS_END_MARKER}",
        crate::RIVUS_PROJECT_TEMPLATE
    )
}

fn rvs_merge_agents_document(existing: &str) -> Result<String, ManagedSectionError> {
    let begin_positions = rvs_marker_positions(existing, RIVUS_AGENTS_BEGIN_MARKER)?;
    let end_positions = rvs_marker_positions(existing, RIVUS_AGENTS_END_MARKER)?;
    let block = rvs_managed_agents_block();

    match (begin_positions.as_slice(), end_positions.as_slice()) {
        ([], []) => {
            let separator = if existing.is_empty() || existing.ends_with("\n\n") {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            Ok(format!("{existing}{separator}{block}\n"))
        }
        ([begin], [end]) if begin < end => {
            let suffix_start = end + RIVUS_AGENTS_END_MARKER.len();
            let prefix =
                existing
                    .get(..*begin)
                    .ok_or_else(|| ManagedSectionError::MarkerConflict {
                        reason: "begin marker is not aligned to a UTF-8 boundary".to_string(),
                        begin: RIVUS_AGENTS_BEGIN_MARKER,
                        end: RIVUS_AGENTS_END_MARKER,
                    })?;
            let suffix = existing.get(suffix_start..).ok_or_else(|| {
                ManagedSectionError::MarkerConflict {
                    reason: "end marker is not aligned to a UTF-8 boundary".to_string(),
                    begin: RIVUS_AGENTS_BEGIN_MARKER,
                    end: RIVUS_AGENTS_END_MARKER,
                }
            })?;
            Ok(format!("{prefix}{block}{suffix}"))
        }
        ([begin], [end]) => Err(ManagedSectionError::MarkerConflict {
            reason: format!("end marker at byte {end} precedes begin marker at byte {begin}"),
            begin: RIVUS_AGENTS_BEGIN_MARKER,
            end: RIVUS_AGENTS_END_MARKER,
        }),
        (begins, ends) => Err(ManagedSectionError::MarkerConflict {
            reason: format!(
                "found {} begin marker(s) and {} end marker(s)",
                begins.len(),
                ends.len()
            ),
            begin: RIVUS_AGENTS_BEGIN_MARKER,
            end: RIVUS_AGENTS_END_MARKER,
        }),
    }
}

pub(crate) fn rvs_run_setup_BIST(path: &Path) -> Result<(), SetupError> {
    let cargo_toml_path = path.join("Cargo.toml");
    let agents_md = path.join("AGENTS.md");
    rvs_preflight_setup_file_BIS(
        &cargo_toml_path,
        "Cargo.toml",
        &SetupFileRequirement::MustExist,
    )?;
    rvs_preflight_setup_file_BIS(&agents_md, "AGENTS.md", &SetupFileRequirement::Optional)?;
    super::workspace::rvs_ensure_cargo_project_BIS(path)
        .map_err(|message| SetupError::ProjectValidation { message })?;

    let project = super::cargo_targets::rvs_load_cargo_project_model_BIS(path)
        .map_err(|message| SetupError::ProjectValidation { message })?;
    super::cargo_targets::rvs_collect_local_crate_prefixes_from_model_BIS(
        path,
        &project,
        super::cargo_targets::CargoTargetScope::WithTestExampleBench,
    )
    .map_err(|message| SetupError::ProjectValidation { message })?;
    let (content, mut document) = project.rvs_into_source_and_document();

    let count = rvs_inject_clippy_lints_into_document_M(&mut document).map_err(|source| {
        SetupError::ClippyConfig {
            path: cargo_toml_path.clone(),
            source,
        }
    })?;
    let new_content = if count > 0 {
        document.to_string()
    } else {
        content.clone()
    };
    let agents_original =
        rvs_read_optional_setup_file_BIS(&agents_md, "AGENTS.md", &SetupFileRequirement::Optional)?;
    let agents_merged = rvs_merge_agents_document(agents_original.as_deref().unwrap_or_default())
        .map_err(|source| SetupError::ManagedDocument {
        path: agents_md.clone(),
        source,
    })?;
    let agents_changed = agents_original.as_deref() != Some(agents_merged.as_str());

    if agents_changed {
        super::fs_guard::rvs_atomic_write_BIST(&agents_md, agents_merged.as_bytes()).map_err(
            |source| SetupError::WriteTarget {
                path: agents_md.clone(),
                source,
            },
        )?;
        println!("Updated managed section in {}", agents_md.display());
    } else {
        println!("Managed section already current in {}", agents_md.display());
    }

    if count > 0 {
        let write_result =
            super::fs_guard::rvs_atomic_write_BIST(&cargo_toml_path, new_content.as_bytes());
        if let Err(source) = write_result {
            let rollback_needed = agents_changed && agents_original.is_some();
            if rollback_needed {
                let original = agents_original.as_deref().unwrap_or_default();
                if let Err(rollback_error) =
                    super::fs_guard::rvs_atomic_write_BIST(&agents_md, original.as_bytes())
                {
                    return Err(SetupError::WriteAndRollback {
                        write_error: source.to_string(),
                        rollback_path: agents_md,
                        rollback_error: rollback_error.to_string(),
                    });
                }
            }
            return Err(SetupError::WriteTarget {
                path: cargo_toml_path,
                source,
            });
        }
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

    Ok(())
}

fn rvs_preflight_setup_file_BIS(
    path: &Path,
    label: &'static str,
    requirement: &SetupFileRequirement,
) -> Result<(), SetupError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SetupError::SymlinkTarget {
            label,
            path: path.to_path_buf(),
        }),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(SetupError::NonRegularTarget {
            label,
            path: path.to_path_buf(),
        }),
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && *requirement == SetupFileRequirement::Optional =>
        {
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SetupError::MissingTarget {
            label,
            path: path.to_path_buf(),
        }),
        Err(source) => Err(SetupError::InspectTarget {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn rvs_read_optional_setup_file_BIS(
    path: &Path,
    label: &'static str,
    requirement: &SetupFileRequirement,
) -> Result<Option<String>, SetupError> {
    rvs_preflight_setup_file_BIS(path, label, requirement)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            let content = super::fs_guard::rvs_read_file_utf8_BIS(path).map_err(|source| {
                SetupError::ReadTarget {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            Ok(Some(content))
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && *requirement == SetupFileRequirement::Optional =>
        {
            Ok(None)
        }
        Err(source) => Err(SetupError::InspectTarget {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    fn rvs_setup_result_debug(result: &Result<(), SetupError>) -> String {
        match result {
            Ok(()) => "Ok(())".to_string(),
            Err(error) => format!("Err({:?})", error.to_string()),
        }
    }

    #[test]
    fn test_20260501_inject_into_empty_cargo_toml() {
        let input = "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints(input).unwrap();
        debug_assert_eq!(count, rvs_clippy_lints().len());
        debug_assert!(result.contains("[lints.clippy]"));
        debug_assert!(result.contains("string_slice = \"warn\""));
        debug_assert!(result.contains("allow_attributes_without_reason = \"warn\""));
    }

    #[test]
    fn test_20260502_inject_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, count1) = rvs_inject_clippy_lints(input).unwrap();
        let (second, count2) = rvs_inject_clippy_lints(&first).unwrap();
        debug_assert!(count1 > 0);
        debug_assert_eq!(count2, 0);
        debug_assert_eq!(first, second);
    }

    #[test]
    fn test_20260503_inject_preserves_existing() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\nunwrap_used = \"warn\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints(input).unwrap();
        debug_assert!(result.contains("string_slice = \"deny\""));
        debug_assert!(result.contains("unwrap_used = \"warn\""));
        debug_assert_eq!(count, rvs_clippy_lints().len() - 2);
    }

    #[test]
    fn test_20260607_setup_inject_clippy_empty() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints(input).unwrap();
        rvs_snapshot_BIS(
            "test_20260607_setup_inject_clippy_empty",
            &format!("count: {count}\n{result}"),
        );
        assert_eq!(count, rvs_clippy_lints().len());
        assert!(result.contains("[lints.clippy]"));
    }

    #[test]
    fn test_20260607_setup_inject_clippy_idempotent() {
        let input = "[package]\nname = \"test\"\n\n[dependencies]\n";
        let (first, c1) = rvs_inject_clippy_lints(input).unwrap();
        let (second, c2) = rvs_inject_clippy_lints(&first).unwrap();
        assert!(c1 > 0);
        assert_eq!(c2, 0);
        assert_eq!(first, second);
    }

    #[test]
    fn test_20260607_setup_inject_clippy_preserves() {
        let input = "[package]\nname = \"test\"\n\n[lints.clippy]\nstring_slice = \"deny\"\n\n[dependencies]\n";
        let (result, count) = rvs_inject_clippy_lints(input).unwrap();
        assert!(result.contains("string_slice = \"deny\""));
        assert_eq!(count, rvs_clippy_lints().len() - 1);
    }

    #[test]
    fn test_20260702_setup_rejects_non_cargo_dir_without_writing_agents() {
        let dir = rvs_make_temp_dir_BIS("setup-non-cargo");
        let agents_md = dir.join("AGENTS.md");
        let result = rvs_run_setup_BIST(&dir);
        let output = format!(
            "result={}\nexists={}\n",
            rvs_setup_result_debug(&result),
            agents_md.exists()
        )
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

        let result = rvs_run_setup_BIST(&dir);
        let output = format!(
            "result={}\nexists={}\n",
            rvs_setup_result_debug(&result),
            agents_md.exists()
        )
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

        let result = rvs_run_setup_BIST(&dir);
        let output = format!(
            "result={}\nexists={}\n",
            rvs_setup_result_debug(&result),
            agents_md.exists()
        )
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

        let result = rvs_run_setup_BIST(&dir);
        let output = format!(
            "result={}\nexists={}\n",
            rvs_setup_result_debug(&result),
            agents_md.exists()
        )
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

        let result = rvs_run_setup_BIST(&dir);
        let restored = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let output = format!(
            "result={}\nrestored={}\nagents_is_dir={}\n",
            rvs_setup_result_debug(&result),
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

    const TEST_MANAGED_BEGIN: &str = "<!-- BEGIN RIVUS MANAGED SECTION: cargo-rivus setup -->";
    const TEST_MANAGED_END: &str = "<!-- END RIVUS MANAGED SECTION: cargo-rivus setup -->";

    fn rvs_write_setup_manifest_BIS(dir: &Path) -> String {
        let cargo_toml =
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n".to_string();
        std::fs::write(dir.join("Cargo.toml"), &cargo_toml).unwrap();
        cargo_toml
    }

    #[test]
    fn test_20260729_public_consumer_template() {
        let template = crate::RIVUS_PROJECT_TEMPLATE;
        rvs_snapshot_BIS("test_20260729_public_consumer_template", template);

        assert!(template.contains("`B/I/P/S/T`"));
        assert!(template.contains("`A/M/U`"));
        assert!(template.contains("World Port"));
        assert!(template.contains("type-level interpreter"));
        assert!(!template.contains("name ends in `Repository`"));
        assert!(template.contains("Result/Option"));
        assert!(!template.contains("~/var/linter-issues/"));
        assert!(!template.contains("开发状态警告（给 LLM）"));
    }

    #[test]
    fn test_20260729_setup_preserves_existing_custom_agents_content() {
        let dir = rvs_make_temp_dir_BIS("setup-preserve-custom-agents");
        rvs_write_setup_manifest_BIS(&dir);
        let custom = "# Team policy\n\nKeep this exact text.\n";
        std::fs::write(dir.join("AGENTS.md"), custom).unwrap();

        let result = rvs_run_setup_BIST(&dir);
        let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let output = format!(
            "ok={}\ncustom_prefix={}\nbegin_markers={}\nend_markers={}\ninternal_policy={}\n",
            result.is_ok(),
            agents.starts_with(custom),
            agents.matches(TEST_MANAGED_BEGIN).count(),
            agents.matches(TEST_MANAGED_END).count(),
            agents.contains("~/var/linter-issues/")
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_preserves_existing_custom_agents_content",
            &output,
        );

        assert!(result.is_ok());
        assert!(agents.starts_with(custom));
        assert_eq!(agents.matches(TEST_MANAGED_BEGIN).count(), 1);
        assert_eq!(agents.matches(TEST_MANAGED_END).count(), 1);
        assert!(!agents.contains("~/var/linter-issues/"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_setup_rejects_malformed_and_duplicate_agents_markers() {
        let cases = [
            (
                "begin_only",
                format!("before\n{TEST_MANAGED_BEGIN}\nmanaged\n"),
            ),
            ("end_only", format!("before\n{TEST_MANAGED_END}\nafter\n")),
            (
                "end_before_begin",
                format!("{TEST_MANAGED_END}\n{TEST_MANAGED_BEGIN}\n"),
            ),
            (
                "duplicate_begin",
                format!("{TEST_MANAGED_BEGIN}\n{TEST_MANAGED_BEGIN}\n{TEST_MANAGED_END}\n"),
            ),
            (
                "duplicate_end",
                format!("{TEST_MANAGED_BEGIN}\n{TEST_MANAGED_END}\n{TEST_MANAGED_END}\n"),
            ),
            (
                "inline_begin",
                format!("prefix {TEST_MANAGED_BEGIN}\n{TEST_MANAGED_END}\n"),
            ),
        ];
        let mut output = String::new();

        for (case, agents_before) in cases {
            let dir = rvs_make_temp_dir_BIS(&format!("setup-markers-{case}"));
            let cargo_before = rvs_write_setup_manifest_BIS(&dir);
            std::fs::write(dir.join("AGENTS.md"), &agents_before).unwrap();

            let result = rvs_run_setup_BIST(&dir);
            let cargo_after = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            let agents_after = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
            let error = result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default();
            output.push_str(&format!(
                "{case}: rejected={} actionable={} cargo_unchanged={} agents_unchanged={}\n",
                result.is_err(),
                error.contains("managed") && error.contains("marker"),
                cargo_after == cargo_before,
                agents_after == agents_before
            ));

            assert!(result.is_err(), "case {case} should be rejected");
            assert!(error.contains("managed") && error.contains("marker"));
            assert_eq!(cargo_after, cargo_before);
            assert_eq!(agents_after == agents_before, true);
            std::fs::remove_dir_all(dir).unwrap();
        }

        rvs_snapshot_BIS(
            "test_20260729_setup_rejects_malformed_and_duplicate_agents_markers",
            &output,
        );
    }

    #[test]
    fn test_20260729_setup_preserves_other_policy_and_config_files() {
        let dir = rvs_make_temp_dir_BIS("setup-preserve-adjacent-policy");
        rvs_write_setup_manifest_BIS(&dir);
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        let files = [
            ("AGENTS.md", "# Existing project policy\n"),
            ("clippy.toml", "msrv = \"1.85\"\n"),
            (".cargo/config", "[term]\nquiet = true\n"),
            (
                ".cargo/config.toml",
                "[build]\nrustflags = [\"-Cdebuginfo=1\"]\n",
            ),
            ("rustfmt.toml", "max_width = 92\n"),
            ("CONTRIBUTING.md", "# Local contribution rules\n"),
        ];
        for (relative, content) in files {
            std::fs::write(dir.join(relative), content).unwrap();
        }

        let result = rvs_run_setup_BIST(&dir);
        let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let adjacent_unchanged = files.iter().skip(1).all(|(relative, content)| {
            std::fs::read_to_string(dir.join(relative)).unwrap() == *content
        });
        let output = format!(
            "ok={}\nagents_custom_preserved={}\nmanaged_markers={}\nadjacent_unchanged={}\n",
            result.is_ok(),
            agents.starts_with(files[0].1),
            agents.matches(TEST_MANAGED_BEGIN).count(),
            adjacent_unchanged
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_preserves_other_policy_and_config_files",
            &output,
        );

        assert!(result.is_ok());
        assert!(agents.starts_with(files[0].1));
        assert_eq!(agents.matches(TEST_MANAGED_BEGIN).count(), 1);
        assert!(adjacent_unchanged);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_setup_replaces_only_existing_managed_region() {
        let dir = rvs_make_temp_dir_BIS("setup-replace-managed-region");
        rvs_write_setup_manifest_BIS(&dir);
        let prefix = "# Team policy: café\r\n\r\n";
        let suffix = "\r\n\r\n# Local footer without final newline";
        let agents_before = format!(
            "{prefix}{TEST_MANAGED_BEGIN}\r\nobsolete managed text\r\n{TEST_MANAGED_END}{suffix}"
        );
        std::fs::write(dir.join("AGENTS.md"), &agents_before).unwrap();

        let result = rvs_run_setup_BIST(&dir);
        let agents_after = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let output = format!(
            "ok={}\nprefix_exact={}\nsuffix_exact={}\nobsolete_removed={}\nbegin_markers={}\nend_markers={}\n",
            result.is_ok(),
            agents_after.starts_with(prefix),
            agents_after.ends_with(suffix),
            !agents_after.contains("obsolete managed text"),
            agents_after.matches(TEST_MANAGED_BEGIN).count(),
            agents_after.matches(TEST_MANAGED_END).count()
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_replaces_only_existing_managed_region",
            &output,
        );

        assert!(result.is_ok());
        assert!(agents_after.starts_with(prefix));
        assert!(agents_after.ends_with(suffix));
        assert!(!agents_after.contains("obsolete managed text"));
        assert_eq!(agents_after.matches(TEST_MANAGED_BEGIN).count(), 1);
        assert_eq!(agents_after.matches(TEST_MANAGED_END).count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260729_setup_merges_inline_clippy_table_structurally() {
        let input = "# root comment\nlints = { clippy = { string_slice = \"deny\" }, rust = { unsafe_code = \"forbid\" } } # lint comment\n\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        let result = rvs_inject_clippy_lints(input);
        let (merged, count) = result.as_ref().expect("inline tables should be mergeable");
        let parsed: DocumentMut = merged.parse().unwrap();
        let output = format!(
            "ok={}\ninserted={}\nroot_comment={}\ninline_comment={}\ncustom_clippy={}\ncustom_rust={}\n",
            result.is_ok(),
            count > &0,
            merged.contains("# root comment"),
            merged.contains("# lint comment"),
            parsed["lints"]["clippy"]["string_slice"].as_str() == Some("deny"),
            parsed["lints"]["rust"]["unsafe_code"].as_str() == Some("forbid")
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_merges_inline_clippy_table_structurally",
            &output,
        );

        assert!(count > &0);
        assert!(merged.contains("# root comment"));
        assert!(merged.contains("# lint comment"));
        assert_eq!(
            parsed["lints"]["clippy"]["string_slice"].as_str(),
            Some("deny")
        );
        assert_eq!(
            parsed["lints"]["rust"]["unsafe_code"].as_str(),
            Some("forbid")
        );
    }

    #[test]
    fn test_20260729_setup_rejects_inherited_workspace_lints() {
        let input = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lints]\nworkspace = true\n";
        let result = rvs_inject_clippy_lints(input);
        let error = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let output = format!(
            "rejected={}\nactionable={}\n",
            result.is_err(),
            error.contains("workspace") && error.contains("[workspace.lints.clippy]")
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_rejects_inherited_workspace_lints",
            &output,
        );

        assert!(result.is_err());
        assert!(error.contains("workspace"));
        assert!(error.contains("[workspace.lints.clippy]"));
    }

    #[test]
    fn test_20260729_setup_repeated_run_is_byte_idempotent() {
        let dir = rvs_make_temp_dir_BIS("setup-repeated-idempotent");
        rvs_write_setup_manifest_BIS(&dir);
        let custom = "# Existing team policy\n";
        std::fs::write(dir.join("AGENTS.md"), custom).unwrap();

        let first_result = rvs_run_setup_BIST(&dir);
        let first_agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let first_cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let second_result = rvs_run_setup_BIST(&dir);
        let second_agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let second_cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let output = format!(
            "first_ok={}\nsecond_ok={}\nagents_identical={}\ncargo_identical={}\ncustom_preserved={}\nbegin_markers={}\nend_markers={}\n",
            first_result.is_ok(),
            second_result.is_ok(),
            first_agents == second_agents,
            first_cargo == second_cargo,
            second_agents.starts_with(custom),
            second_agents.matches(TEST_MANAGED_BEGIN).count(),
            second_agents.matches(TEST_MANAGED_END).count()
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_repeated_run_is_byte_idempotent",
            &output,
        );

        assert!(first_result.is_ok());
        assert!(second_result.is_ok());
        assert_eq!(first_agents, second_agents);
        assert_eq!(first_cargo, second_cargo);
        assert!(second_agents.starts_with(custom));
        assert_eq!(second_agents.matches(TEST_MANAGED_BEGIN).count(), 1);
        assert_eq!(second_agents.matches(TEST_MANAGED_END).count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
