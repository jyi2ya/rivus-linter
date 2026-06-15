//! Workspace-level rename operations for strip and annotate commands.
//!
//! Uses rust-analyzer's `ra_ap_*` crates to load the full workspace,
//! find all function definitions, and perform semantic renames
//! that correctly update all references (including trait impls, macros, etc.).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ra_ap_ide::{
    Analysis, AnalysisHost, FilePosition, FileStructureConfig, Indel, RenameConfig, SourceChange,
    StructureNodeKind,
};
use ra_ap_ide_db::SymbolKind;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::{CargoConfig, RustLibSource};

/// Represents a function/method found via rust-analyzer's file structure.
struct FunctionNode {
    name: String,
    position: FilePosition,
    is_rvs_prefixed: bool,
    is_in_trait_impl: bool,
}

/// Loads the rust-analyzer workspace at `canonical_path` and returns the
/// analysis handle, VFS, and a list of local `.rs` file paths.
fn rvs_load_workspace_BIS(
    canonical_path: &Path,
) -> Result<(Analysis, ra_ap_vfs::Vfs, Vec<PathBuf>), String> {
    let cargo_config = CargoConfig {
        sysroot: Some(RustLibSource::Discover),
        set_test: true,
        ..CargoConfig::default()
    };
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: true,
        with_proc_macro_server: ProcMacroServerChoice::Sysroot,
        prefill_caches: true,
        num_worker_threads: 0,
        proc_macro_processes: 1,
    };

    let (db, vfs, _proc_macro) =
        load_workspace_at(canonical_path, &cargo_config, &load_config, &|_| {})
            .map_err(|e| format!("failed to load workspace: {e}"))?;

    let host = AnalysisHost::with_database(db);
    let analysis = host.analysis();

    let mut local_files: Vec<PathBuf> = Vec::new();
    for (_file_id, vfs_path) in vfs.iter() {
        let raw_path = match vfs_path.as_path() {
            Some(p) => p,
            None => continue,
        };
        let abs_path: &Path = raw_path.as_ref();
        if !abs_path.to_string_lossy().ends_with(".rs") {
            continue;
        }
        if !rvs_is_local_file(abs_path, canonical_path) {
            continue;
        }
        local_files.push(abs_path.to_path_buf());
    }

    Ok((analysis, vfs, local_files))
}

/// Finds all function/method definitions in local files and returns
/// a list of [`FunctionNode`]s with name, position, and context flags.
fn rvs_find_functions_MS(
    analysis: &Analysis,
    vfs: &ra_ap_vfs::Vfs,
    _local_files: &[PathBuf],
    canonical_path: &Path,
) -> Vec<FunctionNode> {
    let mut functions: Vec<FunctionNode> = Vec::new();

    for (file_id, vfs_path) in vfs.iter() {
        let raw_path = match vfs_path.as_path() {
            Some(p) => p,
            None => continue,
        }
        .as_ref();
        let abs_path: &Path = raw_path;
        if !abs_path.to_string_lossy().ends_with(".rs") {
            continue;
        }
        if !rvs_is_local_file(abs_path, canonical_path) {
            continue;
        }

        let structure_config = FileStructureConfig {
            exclude_locals: true,
        };
        let nodes = match analysis.file_structure(&structure_config, file_id) {
            Ok(nodes) => nodes,
            Err(_) => continue,
        };

        let source = match std::fs::read_to_string(abs_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Collect trait impl ranges so we can flag functions inside them.
        let mut trait_impl_ranges: Vec<ra_ap_ide::TextRange> = Vec::new();
        for node in &nodes {
            if let StructureNodeKind::SymbolKind(SymbolKind::Impl) = node.kind {
                if node.label.contains(" for ") {
                    trait_impl_ranges.push(node.node_range);
                }
            }
        }

        for node in &nodes {
            match node.kind {
                StructureNodeKind::SymbolKind(sym) => {
                    if !matches!(sym, SymbolKind::Function | SymbolKind::Method) {
                        continue;
                    }
                }
                _ => continue,
            }

            let nav_start = u32::from(node.navigation_range.start()) as usize;
            let nav_end = u32::from(node.navigation_range.end()) as usize;

            if nav_end > source.len() {
                continue;
            }
            let name = source.get(nav_start..nav_end).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }

            let is_rvs_prefixed = name.starts_with("rvs_");
            let is_in_trait_impl = trait_impl_ranges
                .iter()
                .any(|r| r.contains_range(node.navigation_range));

            functions.push(FunctionNode {
                name,
                position: FilePosition {
                    file_id,
                    offset: node.navigation_range.start(),
                },
                is_rvs_prefixed,
                is_in_trait_impl,
            });
        }
    }

    functions
}

/// Performs semantic renames using rust-analyzer for each entry in `rename_map`.
/// Returns the number of files changed.
fn rvs_apply_renames_BIS(
    analysis: &Analysis,
    vfs: &ra_ap_vfs::Vfs,
    functions: &[FunctionNode],
    rename_map: &HashMap<String, String>,
) -> Result<usize, String> {
    let mut file_edits: HashMap<PathBuf, Vec<Indel>> = HashMap::new();

    let rename_config = RenameConfig {
        prefer_no_std: false,
        prefer_prelude: true,
        prefer_absolute: false,
        show_conflicts: false,
    };

    for func in functions {
        let Some(new_name) = rename_map.get(func.name.as_str()) else {
            continue;
        };

        match analysis.rename(func.position, new_name.as_str(), &rename_config) {
            Ok(Ok(source_change)) => {
                rvs_collect_edits(&source_change, vfs, &mut file_edits);
            }
            Ok(Err(e)) => {
                eprintln!(
                    "warning: RA cannot rename '{}' -> '{}': {e}",
                    func.name, new_name
                );
            }
            Err(e) => {
                eprintln!("warning: RA rename failed for '{}': {e}", func.name);
            }
        }
    }

    let mut files_changed = 0usize;
    for (file_path, mut edits) in file_edits {
        edits.sort_by_key(|e| std::cmp::Reverse(u32::from(e.delete.start())));
        let mut text = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("cannot read {}: {e}", file_path.display()))?;
        for edit in &edits {
            let start: usize = u32::from(edit.delete.start()) as usize;
            let end: usize = u32::from(edit.delete.end()) as usize;
            if end <= text.len() {
                text.replace_range(start..end, &edit.insert);
            }
        }
        std::fs::write(&file_path, &text)
            .map_err(|e| format!("cannot write {}: {e}", file_path.display()))?;
        files_changed += 1;
    }

    Ok(files_changed)
}

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

    let canonical_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;

    // 1. Load workspace
    let (analysis, vfs, _local_files) = rvs_load_workspace_BIS(&canonical_path)?;

    // 2. Find all functions
    let functions = rvs_find_functions_MS(&analysis, &vfs, &_local_files, &canonical_path);

    // 3. Build rename_map from rvs_-prefixed functions
    let mut rename_map: HashMap<String, String> = HashMap::new();
    for func in &functions {
        if !func.is_rvs_prefixed {
            continue;
        }
        if let Some(new_name) = rvs_compute_strip_name(&func.name) {
            if new_name != func.name {
                rename_map.insert(func.name.clone(), new_name);
            }
        }
    }

    if rename_map.is_empty() {
        println!("No rvs_ functions found to strip.");
        return Ok(());
    }

    let rename_count = rename_map.len();

    // 4. Apply semantic renames via rust-analyzer
    let files_changed = rvs_apply_renames_BIS(&analysis, &vfs, &functions, &rename_map)?;

    // 5. Invalidate cached callgraph (function names changed, old cache is stale)
    rvs_invalidate_callgraph_cache_BIS(path);

    // 6. Print summary
    println!(
        "Strip complete: renamed {} function(s) in {} file(s).",
        rename_count, files_changed
    );
    Ok(())
}

/// Applies rust-analyzer semantic renames for non-`rvs_`-prefixed, non-trait-impl
/// functions using the provided rename map.
///
/// Returns the number of files changed.
pub fn rvs_apply_ra_renames_BIS(
    path: &Path,
    rename_map: &HashMap<String, String>,
) -> Result<usize, String> {
    let canonical_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;

    // 1. Load workspace
    let (analysis, vfs, _local_files) = rvs_load_workspace_BIS(&canonical_path)?;

    // 2. Find all functions, then filter: skip rvs_-prefixed and trait impl methods
    let all_functions = rvs_find_functions_MS(&analysis, &vfs, &_local_files, &canonical_path);
    let eligible: Vec<FunctionNode> = all_functions
        .into_iter()
        .filter(|f| !f.is_rvs_prefixed && !f.is_in_trait_impl)
        .collect();

    // 3. Apply semantic renames
    let files_changed = rvs_apply_renames_BIS(&analysis, &vfs, &eligible, rename_map)?;

    // 4. Invalidate cached callgraph (function names changed, old cache is stale)
    if files_changed > 0 {
        rvs_invalidate_callgraph_cache_BIS(path);
    }

    Ok(files_changed)
}

fn rvs_collect_edits(
    source_change: &SourceChange,
    vfs: &ra_ap_vfs::Vfs,
    file_edits: &mut std::collections::HashMap<PathBuf, Vec<Indel>>,
) {
    for (&file_id, (text_edit, _snippet)) in &source_change.source_file_edits {
        let vfs_path = vfs.file_path(file_id);
        let Some(raw_path) = vfs_path.as_path() else {
            continue;
        };
        let abs_path: &Path = raw_path.as_ref();
        let indels: Vec<Indel> = text_edit.iter().cloned().collect();
        if !indels.is_empty() {
            file_edits
                .entry(abs_path.to_path_buf())
                .or_default()
                .extend(indels);
        }
    }
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

/// Removes cached callgraph directories after a rename operation.
/// Function names in the source have changed, so the old callgraph
/// (keyed by function def_path) is stale and must not be reused.
fn rvs_invalidate_callgraph_cache_BIS(project_path: &Path) {
    for dir_name in &["rivus-callgraph", "rivus-callgraph-std"] {
        let dir = project_path.join("target").join(dir_name);
        if dir.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                eprintln!("warning: cannot remove {}: {e}", dir.display());
            }
        }
    }
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
