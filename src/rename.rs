//! Workspace-level rename operations for strip and annotate commands.
//!
//! Uses rust-analyzer's `ra_ap_*` crates to load the full workspace,
//! find all `rvs_` function definitions, and perform semantic renames
//! that correctly update all references (including trait impls, macros, etc.).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ra_ap_ide::{
    AnalysisHost, FilePosition, FileStructureConfig, RenameConfig, SourceChange, StructureNodeKind,
};
use ra_ap_ide_db::FileId;
use ra_ap_ide_db::text_edit::{Indel, TextSize};
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::CargoConfig;
use ra_ap_vfs::Vfs;

/// Strips `rvs_` prefix and capability suffix from all `rvs_` functions in the
/// workspace at `path`, renaming them to their plain base names.
///
/// For example, `rvs_write_db_ABI` becomes `write_db`, `rvs_add` becomes `add`.
///
/// # Errors
///
/// Returns an error string if the workspace cannot be loaded or if file I/O fails.
pub fn rvs_strip_BIS(path: &Path) -> Result<(), String> {
    debug_assert!(path.is_dir(), "path must be a directory");

    let cargo_config = CargoConfig::default();
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: true,
        num_worker_threads: 0,
        proc_macro_processes: 1,
    };

    let (db, vfs, _proc_macro) = load_workspace_at(path, &cargo_config, &load_config, &|_| {})
        .map_err(|e| format!("failed to load workspace: {e}"))?;

    let host = AnalysisHost::with_database(db);
    let analysis = host.analysis();

    let rename_config = RenameConfig {
        prefer_no_std: false,
        prefer_prelude: true,
        prefer_absolute: false,
        show_conflicts: true,
    };

    let mut all_changes: Vec<SourceChange> = Vec::new();

    for (file_id, vfs_path) in vfs.iter() {
        // Only process local .rs files
        let raw_path = match vfs_path.as_path() {
            Some(p) => p,
            None => continue,
        };
        let abs_path: &Path = raw_path.as_ref();
        if !abs_path.to_string_lossy().ends_with(".rs") {
            continue;
        }
        // Skip dependency/external files — only process files under the workspace root
        if !rvs_is_local_file(abs_path, path) {
            continue;
        }

        let structure_config = FileStructureConfig {
            exclude_locals: true,
        };
        let nodes = match analysis.file_structure(&structure_config, file_id) {
            Ok(nodes) => nodes,
            Err(_) => continue,
        };

        for node in &nodes {
            if !node.label.starts_with("rvs_") {
                continue;
            }
            // Only rename functions and methods
            match node.kind {
                StructureNodeKind::SymbolKind(sym) => {
                    if !matches!(
                        sym,
                        ra_ap_ide_db::SymbolKind::Function | ra_ap_ide_db::SymbolKind::Method
                    ) {
                        continue;
                    }
                }
                _ => continue,
            }

            let new_name = match rvs_compute_strip_name(&node.label) {
                Some(name) => name,
                None => continue,
            };

            if new_name == node.label {
                // Nothing to rename (shouldn't happen, but be safe)
                continue;
            }

            // Find the actual name offset in source text (navigation_range can be
            // inaccurate for methods with &self — may include the parameter list)
            let name_offset = match rvs_find_name_offset_BI(
                &node.label,
                u32::from(node.navigation_range.start()),
                &vfs,
                file_id,
            ) {
                Some(offset) => offset,
                None => continue,
            };

            let position = FilePosition {
                file_id,
                offset: TextSize::from(name_offset),
            };

            match analysis.rename(position, &new_name, &rename_config) {
                Ok(Ok(source_change)) => {
                    all_changes.push(source_change);
                }
                Ok(Err(rename_err)) => {
                    eprintln!(
                        "warning: cannot rename '{}' -> '{}': {}",
                        node.label, new_name, rename_err
                    );
                }
                Err(cancelled) => {
                    eprintln!(
                        "warning: rename cancelled for '{}' -> '{}': {cancelled}",
                        node.label, new_name
                    );
                }
            }
        }
    }

    if all_changes.is_empty() {
        println!("No rvs_ functions found to strip.");
        return Ok(());
    }

    let merged = rvs_merge_source_changes(all_changes);
    rvs_apply_all_edits_BIS(&merged, &vfs)?;

    println!("Strip complete: applied edits to {} file(s).", merged.len());
    Ok(())
}

/// Annotates all functions with `rvs_` prefix and capability suffix.
///
/// Currently a stub — will be implemented after strip works.
pub fn rvs_annotate_BIS(_path: &Path) -> Result<(), String> {
    Err("annotate not yet implemented".into())
}

/// Computes the new name for a strip operation.
///
/// Given a function name like `rvs_write_db_ABI`, returns `write_db`.
/// Given `rvs_add`, returns `add`.
/// Returns `None` if the name doesn't start with `rvs_`.
fn rvs_compute_strip_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("rvs_")?;

    // Check if there's a capability suffix after the last underscore
    if let Some(pos) = rest.rfind('_') {
        let potential_suffix = rest.get(pos + 1..).unwrap_or("");
        if !potential_suffix.is_empty() && potential_suffix.chars().all(|c| c.is_ascii_uppercase())
        {
            // Has a suffix — return just the base part
            let base = rest.get(..pos).unwrap_or("");
            return Some(base.to_string());
        }
    }

    // No suffix — just return the part after rvs_
    Some(rest.to_string())
}

/// Checks whether `file_path` is a local file (under `workspace_root`),
/// not a dependency or standard library file.
fn rvs_is_local_file(file_path: &Path, workspace_root: &Path) -> bool {
    // Files under the workspace root are local
    file_path.starts_with(workspace_root)
}

/// Finds the exact byte range of the function name `label` starting near `nav_start`
/// by reading the source file from VFS.
///
/// The `navigation_range` from ra's file_structure can be inaccurate for methods
/// (may include `(&self` in the range). This function searches the actual source
/// text to find the precise range of the name identifier.
fn rvs_find_name_offset_BI(label: &str, nav_start: u32, vfs: &Vfs, file_id: FileId) -> Option<u32> {
    let vfs_path = vfs.file_path(file_id);
    let abs_path = vfs_path.as_path()?;
    let disk_path: &Path = abs_path.as_ref();
    let text = std::fs::read_to_string(disk_path).ok()?;

    // Search for the label near nav_start
    let search_start = nav_start.saturating_sub(5) as usize;
    let search_end = (nav_start as usize + label.len() + 20).min(text.len());
    let window = text.get(search_start..search_end)?;

    if let Some(pos) = window.find(label) {
        let abs_offset = search_start + pos;
        Some(abs_offset as u32)
    } else {
        None
    }
}
///
/// Each file gets all its indels collected and sorted by offset.
fn rvs_merge_source_changes(changes: Vec<SourceChange>) -> HashMap<FileId, Vec<Indel>> {
    let mut per_file: HashMap<FileId, Vec<Indel>> = HashMap::new();

    for change in changes {
        for (file_id, (text_edit, _snippet_edit)) in change.source_file_edits {
            let indels = per_file.entry(file_id).or_default();
            for indel in text_edit {
                indels.push(indel.clone());
            }
        }
        // Ignore file_system_edits for now (renames shouldn't produce them)
    }

    // Deduplicate: same definition (trait + impl) produces identical edits
    for indels in per_file.values_mut() {
        indels.sort_by(|a, b| {
            let a_start: usize = a.delete.start().into();
            let b_start: usize = b.delete.start().into();
            a_start
                .cmp(&b_start)
                .then(usize::from(a.delete.end()).cmp(&usize::from(b.delete.end())))
                .then(a.insert.cmp(&b.insert))
        });
        indels.dedup_by(|a, b| a.delete == b.delete && a.insert == b.insert);
    }

    // Sort each file's indels by offset descending for safe back-to-front application
    for indels in per_file.values_mut() {
        indels.sort_by(|a, b| {
            let a_start: usize = a.delete.start().into();
            let b_start: usize = b.delete.start().into();
            b_start.cmp(&a_start)
        });
    }

    per_file
}

/// Applies all collected edits to the files on disk.
fn rvs_apply_all_edits_BIS(edits: &HashMap<FileId, Vec<Indel>>, vfs: &Vfs) -> Result<(), String> {
    for (&file_id, indels) in edits {
        if indels.is_empty() {
            continue;
        }

        let vfs_path = vfs.file_path(file_id);
        let abs_path = match vfs_path.as_path() {
            Some(p) => p,
            None => {
                eprintln!("warning: skipping file: no local path");
                continue;
            }
        };

        let disk_path: PathBuf = AsRef::<Path>::as_ref(abs_path).to_path_buf();
        let mut text = std::fs::read_to_string(&disk_path)
            .map_err(|e| format!("cannot read {}: {e}", disk_path.display()))?;

        // Apply indels back-to-front (they're already sorted descending by offset)
        for indel in indels {
            let start: usize = indel.delete.start().into();
            let end: usize = indel.delete.end().into();
            text.replace_range(start..end, &indel.insert);
        }

        std::fs::write(&disk_path, &text)
            .map_err(|e| format!("cannot write {}: {e}", disk_path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260610_compute_strip_name_with_suffix() {
        assert_eq!(
            rvs_compute_strip_name("rvs_write_db_ABI"),
            Some("write_db".into())
        );
    }

    #[test]
    fn test_20260610_compute_strip_name_no_suffix() {
        assert_eq!(rvs_compute_strip_name("rvs_add"), Some("add".into()));
    }

    #[test]
    fn test_20260610_compute_strip_name_single_letter_suffix() {
        assert_eq!(
            rvs_compute_strip_name("rvs_sort_inplace_M"),
            Some("sort_inplace".into())
        );
    }

    #[test]
    fn test_20260610_compute_strip_name_bare_rvs() {
        assert_eq!(rvs_compute_strip_name("rvs_"), Some(String::new()));
    }

    #[test]
    fn test_20260610_compute_strip_name_non_rvs() {
        assert_eq!(rvs_compute_strip_name("foo_bar"), None);
    }

    #[test]
    fn test_20260610_compute_strip_name_underscore_in_suffix_not_all_caps() {
        // rvs_foo_ABI1 — "ABI1" is not all uppercase letters
        assert_eq!(
            rvs_compute_strip_name("rvs_foo_ABI1"),
            Some("foo_ABI1".into())
        );
    }

    #[test]
    fn test_20260610_compute_strip_name_long_suffix() {
        assert_eq!(
            rvs_compute_strip_name("rvs_send_email_ABIS"),
            Some("send_email".into())
        );
    }

    #[test]
    fn test_20260610_compute_strip_name_no_suffix_no_underscore() {
        assert_eq!(rvs_compute_strip_name("rvs_parse"), Some("parse".into()));
    }

    #[test]
    fn test_20260610_is_local_file_true() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/project/src/main.rs");
        assert!(rvs_is_local_file(file, root));
    }

    #[test]
    fn test_20260610_is_local_file_false_dependency() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/.cargo/registry/src/some-crate/src/lib.rs");
        assert!(!rvs_is_local_file(file, root));
    }
}
