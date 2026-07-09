use std::collections::BTreeSet;
use std::path::Path;

use crate::symbols::CrateName;

pub(crate) fn rvs_function_matches_local_prefix(
    function: &str,
    local_crate_names: &BTreeSet<CrateName>,
) -> bool {
    local_crate_names
        .iter()
        .any(|name| function.starts_with(name.rvs_prefix().rvs_as_str()))
}

#[cfg(test)]
pub(crate) fn rvs_collect_local_crate_prefixes(toml: &str) -> Result<BTreeSet<CrateName>, String> {
    rvs_collect_local_crate_prefixes_for_targets(toml, true)
}

pub(crate) fn rvs_collect_local_crate_prefixes_for_targets(
    toml: &str,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let doc: toml_edit::DocumentMut = toml.parse().map_err(|e| format!("invalid TOML: {e}"))?;

    let mut prefixes = BTreeSet::new();
    if let Some(package_name) = doc
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[package].name", package_name)?;
    }
    if let Some(lib_name) = doc
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(|name| name.as_str())
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[lib].name", lib_name)?;
    }
    if let Some(bins) = doc.get("bin").and_then(toml_edit::Item::as_array_of_tables) {
        for bin in bins {
            if let Some(name) = bin.get("name").and_then(|name| name.as_str()) {
                rvs_insert_manifest_crate_name_M(&mut prefixes, "[[bin]].name", name)?;
            }
        }
    }
    if include_test_example_bench {
        for table_name in ["test", "example", "bench"] {
            if let Some(targets) = doc
                .get(table_name)
                .and_then(toml_edit::Item::as_array_of_tables)
            {
                for target in targets {
                    if let Some(name) = target.get("name").and_then(|name| name.as_str()) {
                        rvs_insert_manifest_crate_name_M(
                            &mut prefixes,
                            &format!("[[{table_name}]].name"),
                            name,
                        )?;
                    }
                }
            }
        }
    }
    if prefixes.is_empty() {
        return Err(
            "Cargo.toml: missing local crate target ([package].name, [lib].name, [[bin]].name, [[test]].name, [[example]].name, or [[bench]].name)"
                .into(),
        );
    }
    Ok(prefixes)
}

pub(crate) fn rvs_insert_manifest_crate_name_M(
    prefixes: &mut BTreeSet<CrateName>,
    label: &str,
    name: &str,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(format!("{label} must not contain whitespace"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(format!("{label} must be a crate name, not a path"));
    }
    prefixes.insert(CrateName::rvs_from_manifest_name(name));
    Ok(())
}

pub(crate) fn rvs_detect_local_crate_prefixes_BIS(
    path: &Path,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_detect_local_crate_prefixes_for_targets_BIS(path, true)
}

pub(crate) fn rvs_detect_local_crate_prefixes_for_cargo_check_BIS(
    path: &Path,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    rvs_detect_local_crate_prefixes_for_targets_BIS(path, include_test_example_bench)
}

fn rvs_detect_local_crate_prefixes_for_targets_BIS(
    path: &Path,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let mut prefixes =
        rvs_collect_local_crate_prefixes_for_targets(&content, include_test_example_bench)
            .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    rvs_collect_auto_target_prefixes_for_targets_BIMS(
        path,
        &mut prefixes,
        &include_test_example_bench,
    )?;
    Ok(prefixes)
}

#[cfg(test)]
pub(crate) fn rvs_collect_auto_target_prefixes_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    rvs_collect_auto_target_prefixes_for_targets_BIMS(path, prefixes, &true)
}

fn rvs_collect_auto_target_prefixes_for_targets_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
    include_test_example_bench: &bool,
) -> Result<(), String> {
    let flags = rvs_collect_auto_target_flags_BIS(path)?;
    if *include_test_example_bench && flags.autotests {
        let tests_dir = path.join("tests");
        rvs_collect_rs_file_stems_BIMS(&tests_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&tests_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autoexamples {
        let examples_dir = path.join("examples");
        rvs_collect_rs_file_stems_BIMS(&examples_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&examples_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autobenches {
        let benches_dir = path.join("benches");
        rvs_collect_rs_file_stems_BIMS(&benches_dir, prefixes)?;
        rvs_collect_dir_target_names_BIMS(&benches_dir, prefixes)?;
    }
    if !flags.autobins {
        return Ok(());
    }
    let bin_dir = path.join("src/bin");
    rvs_collect_rs_file_stems_BIMS(&bin_dir, prefixes)?;
    rvs_collect_dir_target_names_BIMS(&bin_dir, prefixes)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AutoTargetFlags {
    autobins: bool,
    autotests: bool,
    autoexamples: bool,
    autobenches: bool,
}

fn rvs_collect_auto_target_flags_BIS(path: &Path) -> Result<AutoTargetFlags, String> {
    let mut flags = AutoTargetFlags {
        autobins: true,
        autotests: true,
        autoexamples: true,
        autobenches: true,
    };
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    let Some(package) = doc.get("package") else {
        return Ok(flags);
    };
    if let Some(value) = package.get("autobins").and_then(toml_edit::Item::as_bool) {
        flags.autobins = value;
    }
    if let Some(value) = package.get("autotests").and_then(toml_edit::Item::as_bool) {
        flags.autotests = value;
    }
    if let Some(value) = package
        .get("autoexamples")
        .and_then(toml_edit::Item::as_bool)
    {
        flags.autoexamples = value;
    }
    if let Some(value) = package
        .get("autobenches")
        .and_then(toml_edit::Item::as_bool)
    {
        flags.autobenches = value;
    }
    Ok(flags)
}

pub(crate) fn rvs_collect_rs_file_stems_BIMS(
    dir: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "cannot read auto target dir {}: {e}",
                dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .ok_or_else(|| format!("auto target file has no stem: {}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("auto target file stem is not UTF-8: {}", path.display()))?;
            rvs_insert_manifest_crate_name_M(prefixes, "auto target file stem", stem)?;
        }
    }
    Ok(())
}

pub(crate) fn rvs_collect_dir_target_names_BIMS(
    dir: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "cannot read auto target dir {}: {e}",
                dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.join("main.rs").is_file() {
            let entry_name = entry.file_name();
            let name = entry_name.to_str().ok_or_else(|| {
                format!(
                    "auto target directory name is not UTF-8: {}",
                    path.display()
                )
            })?;
            rvs_insert_manifest_crate_name_M(prefixes, "auto target directory name", name)?;
        }
    }
    Ok(())
}
