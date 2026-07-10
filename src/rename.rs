//! Workspace-level rename operations for strip and annotate commands.
//!
//! Uses rust-analyzer's `ra_ap_*` crates to load the full workspace,
//! find all function definitions, and perform semantic renames
//! that correctly update all references (including trait impls, macros, etc.).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::artifacts::{FnGraph, FnSource};
use crate::capability::rvs_parse_function;
use crate::cargo_targets::rvs_function_matches_local_prefix;
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
    is_in_trait_impl: bool,
}

#[derive(Debug)]
struct LocalVfsFile {
    file_id: FileId,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RenameStats {
    pub(crate) matched_functions: usize,
    pub(crate) files_changed: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceRenameCandidate {
    pub(crate) def_path: DefPath,
    pub(crate) target_name: FnName,
}

impl SourceRenameCandidate {
    pub(crate) fn rvs_new(def_path: DefPath, target_name: FnName) -> Self {
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
    let mut candidates = Vec::new();
    if source.file.is_absolute() {
        candidates.push(source.file.clone());
    } else {
        candidates.push(project_path.join(&source.file));
        if let Some(parent) = project_path.parent() {
            candidates.push(parent.join(&source.file));
        }
    }
    let mut error_count = 0usize;
    let mut file = None;
    for candidate in candidates {
        match candidate.canonicalize() {
            Ok(resolved) => {
                file = Some(resolved);
                break;
            }
            Err(_) => error_count += 1,
        }
    }
    let Some(file) = file else {
        return Err(format!(
            "cannot canonicalize source '{}' from {error_count} candidate path(s)",
            source.file.display(),
        ));
    };
    Ok(FnSource::rvs_new(file, source.name_start, source.name_end))
}

pub(crate) fn rvs_build_source_rename_plan_BIS(
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
                candidate.def_path.rvs_as_str()
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
                candidate.def_path.rvs_as_str()
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

/// Loads the rust-analyzer workspace at `canonical_path` and returns the
/// analysis handle, VFS, and local `.rs` files with their VFS identities.
fn rvs_load_workspace_BIS(
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

fn rvs_is_extra_cargo_target_source(file_path: &Path, canonical_path: &Path) -> bool {
    let Ok(relative) = file_path.strip_prefix(canonical_path) else {
        return false;
    };
    let Some(first) = relative.components().next() else {
        return false;
    };
    let std::path::Component::Normal(name) = first else {
        return false;
    };
    name == "tests" || name == "examples" || name == "benches"
}

/// Finds all function/method definitions in local files and returns
/// a list of [`FunctionNode`]s with name, position, and context flags.
fn rvs_find_functions_BIMS(analysis: &Analysis, local_files: &[LocalVfsFile]) -> Vec<FunctionNode> {
    let mut functions: Vec<FunctionNode> = Vec::new();
    for local_file in local_files {
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

        // Collect trait impl ranges so we can flag functions inside them.
        let mut trait_impl_ranges: Vec<ra_ap_ide::TextRange> = Vec::new();
        for node in &nodes {
            if let StructureNodeKind::SymbolKind(SymbolKind::Impl) = node.kind
                && node.label.contains(" for ")
            {
                trait_impl_ranges.push(node.node_range);
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
            let Some(name_text) = source.get(nav_start..nav_end) else {
                continue;
            };
            let name = FnName::rvs_new(name_text.to_string());
            if name.rvs_as_str().is_empty() {
                continue;
            }

            let is_in_trait_impl = trait_impl_ranges
                .iter()
                .any(|r| r.contains_range(node.navigation_range));
            functions.push(FunctionNode {
                name,
                source: FnSource::rvs_new(
                    local_file.path.clone(),
                    u32::from(node.navigation_range.start()),
                    u32::from(node.navigation_range.end()),
                ),
                position: FilePosition {
                    file_id: local_file.file_id,
                    offset: node.navigation_range.start(),
                },
                is_in_trait_impl,
            });
        }
    }

    functions
}

fn rvs_apply_source_renames_BIS(
    analysis: &Analysis,
    vfs: &ra_ap_vfs::Vfs,
    functions: &[FunctionNode],
    rename_map: &HashMap<FnSource, FnName>,
    canonical_path: &Path,
) -> Result<RenameStats, String> {
    rvs_preflight_source_rename_matches(rename_map, functions)?;
    let mut file_edits: HashMap<PathBuf, Vec<Indel>> = HashMap::new();
    let mut matched_functions = 0usize;

    let rename_config = RenameConfig {
        prefer_no_std: false,
        prefer_prelude: true,
        prefer_absolute: false,
        show_conflicts: false,
    };

    for (source, new_name) in rename_map {
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
                matched_functions = rvs_checked_rename_count(
                    matched_functions,
                    1,
                    "matched rename function count",
                )?;
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

    rvs_write_collected_edits_BIS(file_edits, canonical_path).map(|files_changed| RenameStats {
        matched_functions,
        files_changed,
    })
}

fn rvs_write_collected_edits_BIS(
    file_edits: HashMap<PathBuf, Vec<Indel>>,
    canonical_path: &Path,
) -> Result<usize, String> {
    let mut prepared_files: Vec<(PathBuf, String)> = Vec::new();
    for (file_path, edits) in file_edits {
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

    let mut files_changed = 0usize;
    for (file_path, text) in prepared_files {
        std::fs::write(&file_path, &text)
            .map_err(|e| format!("cannot write {}: {e}", file_path.display()))?;
        files_changed = rvs_checked_rename_count(files_changed, 1, "changed file count")?;
    }

    Ok(files_changed)
}

fn rvs_checked_rename_count(current: usize, delta: usize, label: &str) -> Result<usize, String> {
    debug_assert!(current.checked_add(0).is_some(), "current count is valid");
    debug_assert!(delta.checked_add(0).is_some(), "delta count is valid");
    current
        .checked_add(delta)
        .ok_or_else(|| format!("{label} overflow while applying rust-analyzer renames"))
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
    for key in rename_map.keys() {
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
/// For example, `rvs_write_db_ABI` becomes `write_db`, `rvs_add` becomes `add`.
///
/// # Errors
///
/// Returns an error string if the workspace cannot be loaded or if file I/O fails.
pub fn rvs_strip_BIS(path: &Path) -> Result<(), String> {
    rvs_require_directory_BIS(path)?;
    debug_assert!(path.is_dir(), "path must be a directory");

    let local_crate_names = crate::workspace::rvs_load_local_crate_prefixes_BIS(path)?;
    let callgraph = crate::workspace::rvs_collect_callgraph_BIMS(path, false, false, vec![])?;
    let mut candidates = Vec::new();
    for (def_path, node) in callgraph.rvs_iter() {
        if node.is_trait_impl
            || !rvs_function_matches_local_prefix(def_path.rvs_as_str(), &local_crate_names)
        {
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

    let plan = rvs_build_source_rename_plan_BIS(&callgraph, path, candidates, "strip")?;
    let rename_map = plan.rename_map;

    if rename_map.is_empty() {
        if plan.skipped_without_source > 0 {
            println!(
                "No functions to strip (skipped {} candidate(s) without source metadata).",
                plan.skipped_without_source
            );
            return Ok(());
        }
        println!("No functions to strip.");
        return Ok(());
    }

    let stats = rvs_apply_ra_source_renames_BIS(path, &rename_map)?;

    println!(
        "Strip complete: renamed {} function(s) in {} file(s).",
        stats.matched_functions, stats.files_changed
    );
    Ok(())
}

/// Applies rust-analyzer semantic renames for annotate candidates keyed by exact source location.
///
/// Source-location matching avoids guessing module paths from filenames and disambiguates
/// same-named functions across crate targets.
pub fn rvs_apply_ra_source_renames_BIS(
    path: &Path,
    rename_map: &HashMap<FnSource, FnName>,
) -> Result<RenameStats, String> {
    rvs_require_directory_BIS(path)?;
    let canonical_path = path
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize '{}': {e}", path.display()))?;

    let (analysis, vfs, local_files) = rvs_load_workspace_BIS(&canonical_path)?;
    let all_functions = rvs_find_functions_BIMS(&analysis, &local_files);
    let eligible: Vec<FunctionNode> = all_functions
        .into_iter()
        .filter(|f| {
            !f.is_in_trait_impl
                && !rvs_is_extra_cargo_target_source(&f.source.file, &canonical_path)
        })
        .collect();

    let stats =
        match rvs_apply_source_renames_BIS(&analysis, &vfs, &eligible, rename_map, &canonical_path)
        {
            Ok(stats) => stats,
            Err(e) => {
                if let Err(invalidate_error) = rvs_invalidate_callgraph_cache_BIS(path) {
                    return Err(format!(
                        "{e}; additionally failed to invalidate callgraph cache: {invalidate_error}"
                    ));
                }
                return Err(e);
            }
        };
    if stats.files_changed > 0 {
        rvs_invalidate_callgraph_cache_BIS(path)?;
    }

    Ok(stats)
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
/// Given a function name like `rvs_write_db_ABI`, returns `write_db`.
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

/// Removes cached callgraph directories after a rename operation.
/// Function names in the source have changed, so the old callgraph
/// (keyed by function def_path) is stale and must not be reused.
fn rvs_invalidate_callgraph_cache_BIS(project_path: &Path) -> Result<(), String> {
    for dir_name in &["rivus-callgraph", "rivus-callgraph-std"] {
        let dir = project_path.join("target").join(dir_name);
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?,
            Ok(_) => std::fs::remove_file(&dir)
                .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot inspect {}: {e}", dir.display())),
        }
    }
    Ok(())
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
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};
    use ra_ap_ide::{TextRange, TextSize};

    #[test]
    fn test_20260709_extra_cargo_target_source_filter() {
        let root = Path::new("/workspace/demo");
        let cases = [
            ("src/lib.rs", false),
            ("src/bin/tool.rs", false),
            ("tests/upload_files.rs", true),
            ("tests/fixtures/mod.rs", true),
            ("examples/demo.rs", true),
            ("benches/throughput.rs", true),
            ("../outside/tests/demo.rs", false),
        ];
        let mut output = String::new();
        for (relative, expected) in cases {
            let path = root.join(relative);
            let actual = rvs_is_extra_cargo_target_source(&path, root);
            output.push_str(&format!("{relative}={actual}\n"));
            assert_eq!(actual, expected, "unexpected filter result for {relative}");
        }

        rvs_snapshot_BIS("test_20260709_extra_cargo_target_source_filter", &output);
    }

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
        let dir = rvs_make_temp_dir_BIS("strip-lib-main-same-name");
        let cargo_toml =
            "[package]\nname = \"strip-samename-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn rvs_parse() -> i32 { 1 }\n").unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn rvs_parse() -> i32 { 2 }\n\nfn main() { let _ = rvs_parse(); }\n",
        )
        .unwrap();

        let result = rvs_strip_BIS(&dir);
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
    fn test_20260706_strip_skips_direct_trait_impl_candidates() {
        let dir = rvs_make_temp_dir_BIS("strip-trait-impl-method");
        let cargo_toml =
            "[package]\nname = \"strip-trait-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub trait Api { fn rvs_fetch_P(&self) -> i32; }\npub struct Client;\nimpl Api for Client { fn rvs_fetch_P(&self) -> i32 { 1 } }\npub fn rvs_run_P(api: &dyn Api) -> i32 { api.rvs_fetch_P() }\n",
        )
        .unwrap();

        let result = rvs_strip_BIS(&dir);
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

        let result = rvs_strip_BIS(&dir.join("a"));
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

        let result = rvs_strip_BIS(&cargo_toml);
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
        let overflow = rvs_checked_rename_count(usize::MAX, 1, "demo");
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
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps helper names visible to rivus test-call collection"
    )]
    fn test_20260630_collect_edits_helper_coverage() {
        if std::hint::black_box(false) {
            let _source_change: &SourceChange = unreachable!();
            let _vfs: &ra_ap_vfs::Vfs = unreachable!();
            let _file_edits: &mut std::collections::HashMap<PathBuf, Vec<Indel>> = unreachable!();
            let _source_rename_map: &HashMap<FnSource, FnName> = unreachable!();
            let _functions: &[FunctionNode] = unreachable!();
            let _workspace_root: &Path = unreachable!();
            let _ = rvs_collect_edits_BIMS(_source_change, _vfs, _file_edits, _workspace_root);
            let _ = rvs_preflight_source_rename_matches(_source_rename_map, _functions);
            let _file_path: &Path = unreachable!();
            let _text: &str = unreachable!();
            let _edits: Vec<Indel> = unreachable!();
            let _ = rvs_validate_and_dedup_edits(_file_path, _text, _edits);
        }
    }
}
