//! Workspace-level rename operations for strip and annotate commands.
//!
//! Uses rust-analyzer's `ra_ap_*` crates to load the full workspace,
//! find all function definitions, and perform semantic renames
//! that correctly update all references (including trait impls, macros, etc.).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::artifacts::{FnGraph, FnSource};
use crate::capability::rvs_parse_function;
use crate::function_classification::{FunctionClassification, LocalScope};
use crate::symbols::{DefPath, FnName};

use ra_ap_ide::{
    Analysis, AnalysisHost, FilePosition, FileStructureConfig, Indel, RenameConfig, SourceChange,
    StructureNodeKind,
};
use ra_ap_ide_db::SymbolKind;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::{CargoConfig, RustLibSource};
use ra_ap_vfs::FileId;

/// Represents a function/method found via rust-analyzer's file structure.
#[derive(Debug)]
struct FunctionNode {
    name: FnName,
    source: FnSource,
    position: FilePosition,
}

#[derive(Debug)]
struct LocalVfsFile {
    file_id: FileId,
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceRenameCandidate {
    pub(crate) def_path: DefPath,
    pub(crate) target_name: FnName,
}

impl SourceRenameCandidate {
    pub(crate) const fn rvs_new(def_path: DefPath, target_name: FnName) -> Self {
        Self {
            def_path,
            target_name,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SourceRenamePlan {
    pub(crate) rename_map: HashMap<FnSource, FnName>,
    pub(crate) skipped_without_source: usize,
}

pub(crate) fn rvs_normalize_source_for_project_BIS(
    source: &FnSource,
    project_path: &Path,
) -> Result<FnSource, String> {
    let file = if source.file.is_absolute() {
        if source.base.is_some() {
            return Err(format!(
                "absolute source '{}' must not have a recorded base",
                source.file.display()
            ));
        }
        source.file.canonicalize().map_err(|e| {
            format!(
                "cannot canonicalize source '{}': {e}",
                source.file.display()
            )
        })?
    } else if let Some(base) = &source.base {
        if !base.is_absolute() {
            return Err(format!(
                "recorded source base '{}' for '{}' must be absolute",
                base.display(),
                source.file.display()
            ));
        }
        base.join(&source.file).canonicalize().map_err(|e| {
            format!(
                "cannot canonicalize source '{}' against recorded base '{}': {e}",
                source.file.display(),
                base.display()
            )
        })?
    } else {
        let mut candidate_paths = vec![project_path.join(&source.file)];
        if let Some(parent) = project_path.parent() {
            candidate_paths.push(parent.join(&source.file));
        }
        let candidate_count = candidate_paths.len();
        let mut resolved = Vec::new();
        for candidate in candidate_paths {
            if let Ok(canonical) = candidate.canonicalize()
                && !resolved.contains(&canonical)
            {
                resolved.push(canonical);
            }
        }
        match resolved.as_slice() {
            [file] => file.clone(),
            [] => {
                return Err(format!(
                    "cannot canonicalize legacy source '{}' from {candidate_count} candidate path(s)",
                    source.file.display(),
                ));
            }
            files => {
                return Err(format!(
                    "ambiguous legacy source '{}': {} candidate bases resolve to distinct files [{}]; regenerate callgraph metadata",
                    source.file.display(),
                    files.len(),
                    files
                        .iter()
                        .map(|file| format!("'{}'", file.display()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    };
    Ok(FnSource::rvs_new(file, source.name_start, source.name_end))
}

pub(crate) fn rvs_build_source_rename_plan_BIST(
    graph: &FnGraph,
    project_path: &Path,
    candidates: impl IntoIterator<Item = SourceRenameCandidate>,
    label: &str,
) -> Result<SourceRenamePlan, String> {
    let mut plan = SourceRenamePlan::default();
    for candidate in candidates {
        let Some(node) = graph.rvs_get(candidate.def_path.rvs_as_str()) else {
            plan.skipped_without_source = rvs_checked_rename_count(
                plan.skipped_without_source,
                1,
                "source-less rename skip count",
            )?;
            eprintln!(
                "warning: skipping {label} candidate '{}' because callgraph metadata is missing",
                candidate.def_path
            );
            continue;
        };
        let sources: Vec<_> = node.sources.iter().cloned().collect();
        if sources.is_empty() {
            plan.skipped_without_source = rvs_checked_rename_count(
                plan.skipped_without_source,
                1,
                "source-less rename skip count",
            )?;
            eprintln!(
                "warning: skipping {label} candidate '{}' because it has no real source location metadata",
                candidate.def_path
            );
            continue;
        }
        for source in sources {
            let source = rvs_normalize_source_for_project_BIS(&source, project_path)?;
            if let Some(existing_target_name) = plan.rename_map.get(&source) {
                if existing_target_name != &candidate.target_name {
                    return Err(format!(
                        "{label} candidate source '{}:{}..{}' has conflicting target names ('{}' vs '{}')",
                        source.file.display(),
                        source.name_start,
                        source.name_end,
                        existing_target_name,
                        candidate.target_name
                    ));
                }
                continue;
            }
            plan.rename_map
                .insert(source, candidate.target_name.clone());
        }
    }
    Ok(plan)
}

pub(crate) fn rvs_execute_source_rename_plan_BIST(
    path: &Path,
    plan: &SourceRenamePlan,
    action: &str,
    title: &str,
) -> Result<(), String> {
    if plan.rename_map.is_empty() {
        if plan.skipped_without_source > 0 {
            println!(
                "No functions to {action} (skipped {} candidate(s) without source metadata).",
                plan.skipped_without_source
            );
        } else {
            println!("No functions to {action}.");
        }
        return Ok(());
    }

    let renamed_functions = plan.rename_map.len();
    let files_changed = rvs_apply_ra_source_renames_BIST(path, &plan.rename_map)?;
    println!(
        "{title} complete: renamed {renamed_functions} function(s) in {files_changed} file(s)."
    );
    Ok(())
}

/// Loads the rust-analyzer workspace at `canonical_path` and returns the
/// analysis handle, VFS, and local `.rs` files with their VFS identities.
fn rvs_load_workspace_BIST(
    canonical_path: &Path,
) -> Result<(Analysis, ra_ap_vfs::Vfs, Vec<LocalVfsFile>), String> {
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

    let mut local_files: Vec<LocalVfsFile> = Vec::new();
    for (file_id, vfs_path) in vfs.iter() {
        let raw_path = match vfs_path.as_path() {
            Some(p) => p,
            None => continue,
        };
        let abs_path: &Path = raw_path.as_ref();
        if !abs_path.to_string_lossy().ends_with(".rs") {
            continue;
        }
        let Some(path) = rvs_canonical_local_file_BIS(abs_path, canonical_path) else {
            continue;
        };
        local_files.push(LocalVfsFile { file_id, path });
    }

    Ok((analysis, vfs, local_files))
}

/// Resolves source-plan entries to rust-analyzer function positions.
fn rvs_find_functions_BIST(
    analysis: &Analysis,
    local_files: &[LocalVfsFile],
    rename_map: &HashMap<FnSource, FnName>,
) -> Vec<FunctionNode> {
    let mut functions: Vec<FunctionNode> = Vec::new();
    for local_file in local_files {
        if !rename_map
            .keys()
            .any(|source| source.file == local_file.path)
        {
            continue;
        }
        let structure_config = FileStructureConfig {
            exclude_locals: true,
        };
        let nodes = match analysis.file_structure(&structure_config, local_file.file_id) {
            Ok(nodes) => nodes,
            Err(_) => continue,
        };

        let source = match std::fs::read_to_string(&local_file.path) {
            Ok(s) => s,
            Err(_) => continue,
        };

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
            let Some(name_text) = source.get(nav_start..nav_end) else {
                continue;
            };
            let name = FnName::rvs_new(name_text.to_string());
            if name.rvs_as_str().is_empty() {
                continue;
            }

            let source = FnSource::rvs_new(
                local_file.path.clone(),
                u32::from(node.navigation_range.start()),
                u32::from(node.navigation_range.end()),
            );
            if !rename_map.contains_key(&source) {
                continue;
            }
            functions.push(FunctionNode {
                name,
                source,
                position: FilePosition {
                    file_id: local_file.file_id,
                    offset: node.navigation_range.start(),
                },
            });
        }
    }

    functions
}

fn rvs_apply_source_renames_BIST(
    analysis: &Analysis,
    vfs: &ra_ap_vfs::Vfs,
    functions: &[FunctionNode],
    rename_map: &HashMap<FnSource, FnName>,
    canonical_path: &Path,
) -> Result<usize, String> {
    rvs_preflight_source_rename_matches(rename_map, functions)?;
    let mut file_edits: HashMap<PathBuf, Vec<Indel>> = HashMap::new();

    let rename_config = RenameConfig {
        prefer_no_std: false,
        prefer_prelude: true,
        prefer_absolute: false,
        show_conflicts: false,
    };

    let mut ordered_renames = rename_map.iter().collect::<Vec<_>>();
    ordered_renames.sort_by_key(|(source, _)| *source);
    for (source, new_name) in ordered_renames {
        let func = functions
            .iter()
            .find(|func| &func.source == source)
            .expect("never: source rename preflight guarantees exactly one matching function");
        match analysis.rename(func.position, new_name.rvs_as_str(), &rename_config) {
            Ok(Ok(source_change)) => {
                let edits =
                    rvs_collect_edits_BIMS(&source_change, vfs, &mut file_edits, canonical_path)?;
                if edits == 0 {
                    return Err(format!(
                        "rust-analyzer produced no edits for '{}' -> '{}'",
                        func.name, new_name
                    ));
                }
            }
            Ok(Err(e)) => {
                return Err(format!(
                    "rust-analyzer cannot rename '{}' -> '{}': {e}",
                    func.name, new_name
                ));
            }
            Err(e) => {
                return Err(format!(
                    "rust-analyzer rename failed for '{}': {e}",
                    func.name
                ));
            }
        }
    }

    rvs_write_collected_edits_BIS(file_edits, canonical_path)
}

fn rvs_write_collected_edits_BIS(
    file_edits: HashMap<PathBuf, Vec<Indel>>,
    canonical_path: &Path,
) -> Result<usize, String> {
    let mut ordered_file_edits = file_edits.into_iter().collect::<Vec<_>>();
    ordered_file_edits.sort_by(|(left, _), (right, _)| left.cmp(right));
    let mut prepared_files: Vec<(PathBuf, String)> = Vec::new();
    for (file_path, edits) in ordered_file_edits {
        rvs_require_local_real_file_BIS(&file_path, canonical_path)?;
        let mut text = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("cannot read {}: {e}", file_path.display()))?;
        let edits = rvs_validate_and_dedup_edits(&file_path, &text, edits)?;
        for edit in edits.iter().rev() {
            let start: usize = u32::from(edit.delete.start()) as usize;
            let end: usize = u32::from(edit.delete.end()) as usize;
            text.replace_range(start..end, &edit.insert);
        }
        prepared_files.push((file_path, text));
    }

    let files_changed = prepared_files.len();
    for (file_path, text) in prepared_files {
        std::fs::write(&file_path, &text)
            .map_err(|e| format!("cannot write {}: {e}", file_path.display()))?;
    }

    Ok(files_changed)
}

fn rvs_checked_rename_count(current: usize, delta: usize, label: &str) -> Result<usize, String> {
    debug_assert!(current < usize::MAX, "running total must not be saturated");
    debug_assert!(delta < usize::MAX, "increment must not be saturated");
    super::rvs_checked_count_sum(current, delta, label)
}

fn rvs_require_local_real_file_BIS(file_path: &Path, canonical_path: &Path) -> Result<(), String> {
    let real_path = file_path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize edit file {}: {e}", file_path.display()))?;
    if !real_path.starts_with(canonical_path) {
        return Err(format!(
            "rust-analyzer produced edit outside project root: {} resolves to {}",
            file_path.display(),
            real_path.display()
        ));
    }
    let target_dir = canonical_path.join("target");
    if real_path.starts_with(&target_dir) {
        return Err(format!(
            "rust-analyzer produced edit inside target directory: {} resolves to {}",
            file_path.display(),
            real_path.display()
        ));
    }
    Ok(())
}

fn rvs_preflight_source_rename_matches(
    rename_map: &HashMap<FnSource, FnName>,
    functions: &[FunctionNode],
) -> Result<(), String> {
    let mut keys = rename_map.keys().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        let matches = functions.iter().filter(|func| &func.source == key).count();
        match matches {
            1 => {}
            0 => {
                return Err(format!(
                    "rename candidate '{}:{}..{}' did not match any rust-analyzer symbol",
                    key.file.display(),
                    key.name_start,
                    key.name_end
                ));
            }
            _ => {
                return Err(format!(
                    "rename candidate '{}:{}..{}' matched {matches} rust-analyzer symbols",
                    key.file.display(),
                    key.name_start,
                    key.name_end
                ));
            }
        }
    }
    Ok(())
}

/// Strips `rvs_` prefix and capability suffix from all `rvs_` functions in the
/// workspace at `path`, renaming them to their plain base names.
///
/// For example, `rvs_write_db_BIS` becomes `write_db`, `rvs_add` becomes `add`.
///
/// # Errors
///
/// Returns an error string if the workspace cannot be loaded or if file I/O fails.
pub fn rvs_strip_BIST(path: &Path) -> Result<(), String> {
    rvs_require_directory_BIS(path)?;
    debug_assert!(path.is_dir(), "path must be a directory");
    let project_path = super::workspace::rvs_canonical_cargo_project_BIS(path)?;

    let target_scope = super::cargo_targets::CargoTargetScope::Production;
    let local_crate_names =
        super::workspace::rvs_load_local_crate_prefixes_BIS(&project_path, target_scope)?;
    let callgraph = super::workspace::rvs_collect_workspace_callgraph_BIST(
        &project_path,
        target_scope,
        &local_crate_names,
    )?;
    let mut candidates = Vec::new();
    let scope = LocalScope::rvs_for_graph(&local_crate_names, &callgraph);
    for (def_path, node) in callgraph.rvs_iter() {
        if !FunctionClassification::rvs_new(&scope, def_path, node).rvs_is_strip_candidate() {
            continue;
        }
        let current_name = def_path.rvs_fn_name();
        if let Some(new_name) = rvs_compute_strip_name(current_name.rvs_as_str())
            && !new_name.is_empty()
            && new_name != current_name.rvs_as_str()
        {
            candidates.push(SourceRenameCandidate::rvs_new(
                def_path.clone(),
                FnName::rvs_new(new_name),
            ));
        }
    }

    let plan = rvs_build_source_rename_plan_BIST(&callgraph, &project_path, candidates, "strip")?;
    rvs_execute_source_rename_plan_BIST(&project_path, &plan, "strip", "Strip")
}

/// Applies rust-analyzer semantic renames for annotate candidates keyed by exact source location.
///
/// Source-location matching avoids guessing module paths from filenames and disambiguates
/// same-named functions across crate targets.
pub fn rvs_apply_ra_source_renames_BIST(
    path: &Path,
    rename_map: &HashMap<FnSource, FnName>,
) -> Result<usize, String> {
    rvs_require_directory_BIS(path)?;
    let canonical_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;

    let (analysis, vfs, local_files) = rvs_load_workspace_BIST(&canonical_path)?;
    let functions = rvs_find_functions_BIST(&analysis, &local_files, rename_map);

    let files_changed = match rvs_apply_source_renames_BIST(
        &analysis,
        &vfs,
        &functions,
        rename_map,
        &canonical_path,
    ) {
        Ok(files_changed) => files_changed,
        Err(e) => {
            if let Err(invalidate_error) = rvs_invalidate_callgraph_cache_BIS(path) {
                return Err(format!(
                    "{e}; additionally failed to invalidate callgraph cache: {invalidate_error}"
                ));
            }
            return Err(e);
        }
    };
    if files_changed > 0 {
        rvs_invalidate_callgraph_cache_BIS(path)?;
    }

    Ok(files_changed)
}

fn rvs_collect_edits_BIMS(
    source_change: &SourceChange,
    vfs: &ra_ap_vfs::Vfs,
    file_edits: &mut std::collections::HashMap<PathBuf, Vec<Indel>>,
    canonical_path: &Path,
) -> Result<usize, String> {
    let mut edits_added = 0usize;
    for (&file_id, (text_edit, _snippet)) in &source_change.source_file_edits {
        let vfs_path = vfs.file_path(file_id);
        let raw_path = vfs_path
            .as_path()
            .ok_or_else(|| format!("rust-analyzer edit has no filesystem path: {vfs_path:?}"))?;
        let abs_path: &Path = raw_path.as_ref();
        if !rvs_is_local_file_BIS(abs_path, canonical_path) {
            return Err(format!(
                "rust-analyzer produced edit outside project root: {}",
                abs_path.display()
            ));
        }
        let indels: Vec<Indel> = text_edit.iter().cloned().collect();
        if !indels.is_empty() {
            edits_added =
                rvs_checked_rename_count(edits_added, indels.len(), "rust-analyzer edit count")?;
            file_edits
                .entry(abs_path.to_path_buf())
                .or_default()
                .extend(indels);
        }
    }
    Ok(edits_added)
}

fn rvs_validate_and_dedup_edits(
    file_path: &Path,
    text: &str,
    mut edits: Vec<Indel>,
) -> Result<Vec<Indel>, String> {
    edits.sort_by_key(|edit| (u32::from(edit.delete.start()), u32::from(edit.delete.end())));
    let mut deduped: Vec<Indel> = Vec::new();
    let mut previous_end = 0usize;
    for edit in edits {
        let start: usize = u32::from(edit.delete.start()) as usize;
        let end: usize = u32::from(edit.delete.end()) as usize;
        if start > end {
            return Err(format!(
                "invalid rust-analyzer edit in {}: start {start} > end {end}",
                file_path.display()
            ));
        }
        if end > text.len() {
            return Err(format!(
                "invalid rust-analyzer edit in {}: range {start}..{end} exceeds file length {}",
                file_path.display(),
                text.len()
            ));
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(format!(
                "invalid rust-analyzer edit in {}: range {start}..{end} is not on UTF-8 boundaries",
                file_path.display()
            ));
        }
        if let Some(last) = deduped.last() {
            let last_start: usize = u32::from(last.delete.start()) as usize;
            let last_end: usize = u32::from(last.delete.end()) as usize;
            if last_start == start && last_end == end && last.insert == edit.insert {
                continue;
            }
        }
        if start < previous_end {
            return Err(format!(
                "overlapping rust-analyzer edits in {} near byte {start}",
                file_path.display()
            ));
        }
        previous_end = end;
        deduped.push(edit);
    }
    Ok(deduped)
}

/// Computes the new name for a strip operation.
///
/// Given a function name like `rvs_write_db_BIS`, returns `write_db`.
/// Given `rvs_add`, returns `add`.
/// Returns `None` if the name doesn't start with `rvs_`.
fn rvs_compute_strip_name(name: &str) -> Option<String> {
    let (base, _) = rvs_parse_function(name)?;
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

/// Checks whether `file_path` is a local file (under `workspace_root`),
/// not a dependency or standard library file.
fn rvs_is_local_file_BIS(file_path: &Path, workspace_root: &Path) -> bool {
    rvs_canonical_local_file_BIS(file_path, workspace_root).is_some()
}

fn rvs_canonical_local_file_BIS(
    file_path: &Path,
    canonical_workspace_root: &Path,
) -> Option<PathBuf> {
    let Ok(real_file) = file_path.canonicalize() else {
        return None;
    };
    if !real_file.starts_with(canonical_workspace_root)
        || real_file.starts_with(canonical_workspace_root.join("target"))
    {
        return None;
    }
    Some(real_file)
}

/// Removes published and legacy callgraph caches after a rename operation.
/// Function names in the source have changed, so the old callgraph
/// (keyed by function def_path) is stale and must not be reused.
fn rvs_invalidate_callgraph_cache_BIS(project_path: &Path) -> Result<(), String> {
    let target = project_path.join("target");
    let caches = [
        target.join("rivus-callgraph"),
        super::callgraph_cache::rvs_std_callgraph_cache_dir(project_path),
        super::callgraph_cache::rvs_std_callgraph_cache_path(project_path),
    ];
    rvs_invalidate_callgraph_cache_paths_BIS(&caches)
}

fn rvs_invalidate_callgraph_cache_paths_BIS(caches: &[PathBuf]) -> Result<(), String> {
    let mut errors = Vec::new();
    for cache in caches {
        if let Err(error) = super::workspace::rvs_clean_dir_BIS(cache) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn rvs_require_directory_BIS(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!("path must be a directory: {}", path.display())),
        Err(e) => Err(format!("cannot inspect '{}': {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS, rvs_register_test_coverage,
        rvs_snapshot_BIS,
    };
    use ra_ap_ide::{TextRange, TextSize};

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
        assert_eq!(rvs_compute_strip_name("rvs_"), None);
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
        let root = rvs_make_temp_dir_BIS("local-file-true");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert!(rvs_is_local_file_BIS(&file, &root));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_20260610_is_local_file_false_dependency() {
        let root = rvs_make_temp_dir_BIS("local-file-false-dep-root");
        let dep = rvs_make_temp_dir_BIS("local-file-false-dep");
        std::fs::create_dir_all(dep.join("src")).unwrap();
        let file = dep.join("src/lib.rs");
        std::fs::write(&file, "pub fn rvs_dep() {}\n").unwrap();
        assert!(!rvs_is_local_file_BIS(&file, &root));
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(dep).unwrap();
    }

    #[test]
    fn test_20260705_is_local_file_false_target_dir() {
        let root = rvs_make_temp_dir_BIS("local-file-target");
        let file = root.join("target/debug/build/pkg/out/generated.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "pub fn rvs_generated() {}\n").unwrap();
        let is_local = rvs_is_local_file_BIS(&file, &root);
        rvs_snapshot_BIS(
            "test_20260705_is_local_file_false_target_dir",
            &format!("is_local={is_local}\n"),
        );
        assert!(!is_local);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260707_is_local_file_rejects_symlink_escape() {
        let root = rvs_make_temp_dir_BIS("local-file-symlink-root");
        let external = rvs_make_temp_dir_BIS("local-file-symlink-external");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let external_file = external.join("outside.rs");
        std::fs::write(&external_file, "pub fn rvs_outside() {}\n").unwrap();
        let link = root.join("src/outside.rs");
        std::os::unix::fs::symlink(&external_file, &link).unwrap();

        let is_local = rvs_is_local_file_BIS(&link, &root);
        rvs_snapshot_BIS(
            "test_20260707_is_local_file_rejects_symlink_escape",
            &format!("is_local={is_local}\n"),
        );

        assert!(!is_local);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn test_20260706_strip_renames_lib_and_main_same_name_functions() {
        let dir = rvs_make_cargo_project_BIS(
            "strip-lib-main-same-name",
            "strip-samename-demo",
            &[
                ("src/lib.rs", "pub fn rvs_parse() -> i32 { 1 }\n"),
                (
                    "src/main.rs",
                    "fn rvs_parse() -> i32 { 2 }\n\nfn main() { let _ = rvs_parse(); }\n",
                ),
            ],
        );

        let result = rvs_strip_BIST(&dir);
        let lib_source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let main_source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_strip_renames_lib_and_main_same_name_functions",
            &format!("lib:\n{lib_source}\nmain:\n{main_source}"),
        );

        assert!(result.is_ok(), "strip should succeed: {result:?}");
        assert!(lib_source.contains("pub fn parse()"));
        assert!(main_source.contains("fn parse()"));
        assert!(main_source.contains("parse();"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_strip_renames_production_module_under_tests_directory() {
        let dir = rvs_make_cargo_project_BIS(
            "strip-production-module-under-tests",
            "strip-production-path-demo",
            &[
                (
                    "src/lib.rs",
                    "#[path = \"../tests/production.rs\"]\nmod production;\npub use production::rvs_parse;\n",
                ),
                ("tests/production.rs", "pub fn rvs_parse() -> i32 { 1 }\n"),
            ],
        );

        let result = rvs_strip_BIST(&dir);
        let lib_source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let module_source = std::fs::read_to_string(dir.join("tests/production.rs")).unwrap();
        let output = format!(
            "result_is_ok={}\nlib:\n{lib_source}\nmodule:\n{module_source}",
            result.is_ok()
        );
        rvs_snapshot_BIS(
            "test_20260713_strip_renames_production_module_under_tests_directory",
            &output,
        );

        assert!(result.is_ok(), "strip should succeed: {result:?}");
        assert!(lib_source.contains("pub use production::parse;"));
        assert!(module_source.contains("pub fn parse()"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_strip_skips_direct_trait_impl_candidates() {
        let dir = rvs_make_cargo_project_BIS(
            "strip-trait-impl-method",
            "strip-trait-demo",
            &[(
                "src/lib.rs",
                "pub trait Api { fn rvs_fetch_P(&self) -> i32; }\npub struct Client;\nimpl Api for Client { fn rvs_fetch_P(&self) -> i32 { 1 } }\npub fn rvs_run_P(api: &dyn Api) -> i32 { api.rvs_fetch_P() }\n",
            )],
        );

        let result = rvs_strip_BIST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_strip_skips_direct_trait_impl_candidates",
            &source,
        );

        assert!(result.is_ok(), "strip should succeed: {result:?}");
        assert!(source.contains("fn fetch(&self) -> i32;"));
        assert!(source.contains("fn fetch(&self) -> i32 { 1 }"));
        assert!(source.contains("api.fetch()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_strip_does_not_edit_sibling_workspace_member() {
        let dir = rvs_make_temp_dir_BIS("strip-sibling-member");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        for member in ["a", "b"] {
            std::fs::create_dir_all(dir.join(member).join("src")).unwrap();
        }
        std::fs::write(
            dir.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b/Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a/src/lib.rs"),
            "pub fn rvs_parse() -> i32 { 1 }\n",
        )
        .unwrap();
        let b_before = "pub fn run() -> i32 { a::rvs_parse() }\n";
        std::fs::write(dir.join("b/src/lib.rs"), b_before).unwrap();

        let result = rvs_strip_BIST(&dir.join("a"));
        let b_after = std::fs::read_to_string(dir.join("b/src/lib.rs")).unwrap();
        let output = format!("result={result:?}\nb_after={b_after}");
        rvs_snapshot_BIS(
            "test_20260706_strip_does_not_edit_sibling_workspace_member",
            &output.replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert_eq!(b_after, b_before);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_strip_rejects_file_path_before_loading_workspace() {
        let dir = rvs_make_temp_dir_BIS("strip-file-path");
        let cargo_toml = dir.join("Cargo.toml");
        std::fs::write(
            &cargo_toml,
            "[package]\nname = \"strip-file-path\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        let result = rvs_strip_BIST(&cargo_toml);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_strip_rejects_file_path_before_loading_workspace",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("path must be a directory"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_collected_edits_rejects_symlink_escape() {
        let dir = rvs_make_temp_dir_BIS("edit-symlink-project");
        let outside = rvs_make_temp_dir_BIS("edit-symlink-outside");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let outside_file = outside.join("lib.rs");
        let before = "pub fn rvs_escape() {}\n";
        std::fs::write(&outside_file, before).unwrap();
        let symlink_path = dir.join("src/lib.rs");
        std::os::unix::fs::symlink(&outside_file, &symlink_path).unwrap();
        let edits = HashMap::from([(
            symlink_path,
            vec![Indel {
                delete: TextRange::new(TextSize::from(7), TextSize::from(17)),
                insert: "escape".to_string(),
            }],
        )]);

        let canonical_path = dir.canonicalize().unwrap();
        let result = rvs_write_collected_edits_BIS(edits, &canonical_path);
        let after = std::fs::read_to_string(&outside_file).unwrap();
        let output = format!("result={result:?}\nafter={after}")
            .replace(&dir.to_string_lossy().into_owned(), "$PROJECT")
            .replace(&outside.to_string_lossy().into_owned(), "$OUTSIDE");
        rvs_snapshot_BIS(
            "test_20260706_write_collected_edits_rejects_symlink_escape",
            &output,
        );

        assert!(result.is_err());
        assert_eq!(after, before);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn test_20260706_checked_rename_count_handles_ok_and_overflow() {
        let ok = rvs_checked_rename_count(4, 5, "demo");
        // Non-saturated operands whose sum still overflows: exercises the
        // labeled overflow error without tripping the saturation sentinel
        // precondition.
        let overflow = rvs_checked_rename_count(usize::MAX - 2, 5, "demo");
        rvs_snapshot_BIS(
            "test_20260706_checked_rename_count_handles_ok_and_overflow",
            &format!("ok={ok:?}\noverflow={overflow:?}\n"),
        );

        assert_eq!(ok, Ok(9));
        assert!(overflow.is_err());
    }

    #[test]
    fn test_20260706_invalidate_callgraph_cache_removes_file_path() {
        let dir = rvs_make_temp_dir_BIS("invalidate-cache-file");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let cache = dir.join("target/rivus-callgraph");
        std::fs::write(&cache, "stale").unwrap();

        let result = rvs_invalidate_callgraph_cache_BIS(&dir);
        let exists = cache.exists();
        rvs_snapshot_BIS(
            "test_20260706_invalidate_callgraph_cache_removes_file_path",
            &format!("result={result:?}\nexists={exists}\n"),
        );

        assert!(result.is_ok());
        assert!(!exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_invalidate_callgraph_cache_removes_broken_symlink() {
        let dir = rvs_make_temp_dir_BIS("invalidate-cache-symlink");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let cache = dir.join("target/rivus-callgraph");
        std::os::unix::fs::symlink(dir.join("missing"), &cache).unwrap();

        let result = rvs_invalidate_callgraph_cache_BIS(&dir);
        let exists = std::fs::symlink_metadata(&cache).is_ok();
        rvs_snapshot_BIS(
            "test_20260706_invalidate_callgraph_cache_removes_broken_symlink",
            &format!("result={result:?}\nexists={exists}\n"),
        );

        assert!(result.is_ok());
        assert!(!exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_invalidate_callgraph_cache_preserves_active_generation() {
        let dir = rvs_make_temp_dir_BIS("invalidate-published-cache");
        let active_generation = dir.join("target/.rivus-runs/callgraph-active");
        std::fs::create_dir_all(&active_generation).unwrap();
        std::fs::write(active_generation.join("sentinel"), "active\n").unwrap();
        let published_cache = dir.join("target/rivus-callgraph-std.json");
        std::fs::write(&published_cache, "stale\n").unwrap();

        let result = rvs_invalidate_callgraph_cache_BIS(&dir);
        let published_removed = !published_cache.exists();
        let active_preserved = active_generation.join("sentinel").is_file();
        rvs_snapshot_BIS(
            "test_20260715_invalidate_callgraph_cache_preserves_active_generation",
            &format!(
                "result={result:?}\npublished_removed={published_removed}\nactive_preserved={active_preserved}\n"
            ),
        );

        assert!(result.is_ok());
        assert!(published_removed);
        assert!(active_preserved);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_cache_invalidation_continues_after_error() {
        use std::os::unix::ffi::OsStringExt;

        let dir = rvs_make_temp_dir_BIS("invalidate-cache-continues");
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"invalid\0path".to_vec()));
        let removable = dir.join("removable-cache");
        std::fs::write(&removable, "stale\n").unwrap();

        let result = rvs_invalidate_callgraph_cache_paths_BIS(&[invalid, removable.clone()]);
        let later_removed = !removable.exists();
        rvs_snapshot_BIS(
            "test_20260715_cache_invalidation_continues_after_error",
            &format!(
                "result_is_err={}\nlater_removed={later_removed}\n",
                result.is_err()
            ),
        );

        assert!(result.is_err());
        assert!(later_removed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260714_source_rename_preflight_reports_first_sorted_candidate() {
        let earlier = FnSource::rvs_new(PathBuf::from("a.rs"), 10, 20);
        let later = FnSource::rvs_new(PathBuf::from("z.rs"), 30, 40);
        let rename_map = HashMap::from([
            (later, FnName::from("rvs_later")),
            (earlier, FnName::from("rvs_earlier")),
        ]);

        let result = rvs_preflight_source_rename_matches(&rename_map, &[]);
        rvs_snapshot_BIS(
            "test_20260714_source_rename_preflight_reports_first_sorted_candidate",
            &format!("{result:?}\n"),
        );

        assert!(
            result
                .as_ref()
                .is_err_and(|message| message.contains("a.rs:10..20"))
        );
    }

    #[test]
    fn test_20260630_collect_edits_helper_coverage() {
        rvs_register_test_coverage((
            rvs_collect_edits_BIMS,
            rvs_preflight_source_rename_matches,
            rvs_validate_and_dedup_edits,
        ));
    }
}
