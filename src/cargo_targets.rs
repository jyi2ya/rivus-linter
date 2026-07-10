use std::collections::BTreeSet;
use std::path::Path;

use crate::symbols::CrateName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoTargetFlags {
    autobins: bool,
    autotests: bool,
    autoexamples: bool,
    autobenches: bool,
}

#[derive(Debug)]
pub(crate) struct CargoProjectModel {
    source: String,
    document: toml_edit::DocumentMut,
}

impl CargoProjectModel {
    pub(crate) fn rvs_into_source_and_document(self) -> (String, toml_edit::DocumentMut) {
        (self.source, self.document)
    }
}

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

#[cfg(test)]
pub(crate) fn rvs_collect_local_crate_prefixes_for_targets(
    toml: &str,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let model = rvs_parse_cargo_project_model(toml).map_err(|e| format!("invalid TOML: {e}"))?;
    rvs_collect_manifest_crate_prefixes(&model.document, include_test_example_bench)
}

fn rvs_collect_manifest_crate_prefixes(
    document: &toml_edit::DocumentMut,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let mut prefixes = BTreeSet::new();
    if let Some(package_name) = document
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml_edit::Item::as_str)
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[package].name", package_name)?;
    }
    if let Some(lib_name) = document
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(toml_edit::Item::as_str)
    {
        rvs_insert_manifest_crate_name_M(&mut prefixes, "[lib].name", lib_name)?;
    }
    if let Some(bins) = document
        .get("bin")
        .and_then(toml_edit::Item::as_array_of_tables)
    {
        for bin in bins {
            if let Some(name) = bin.get("name").and_then(toml_edit::Item::as_str) {
                rvs_insert_manifest_crate_name_M(&mut prefixes, "[[bin]].name", name)?;
            }
        }
    }
    if include_test_example_bench {
        for table_name in ["test", "example", "bench"] {
            if let Some(targets) = document
                .get(table_name)
                .and_then(toml_edit::Item::as_array_of_tables)
            {
                for target in targets {
                    if let Some(name) = target.get("name").and_then(toml_edit::Item::as_str) {
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
    let model = rvs_load_cargo_project_model_BIS(path)?;
    rvs_collect_local_crate_prefixes_from_model_BIS(path, &model, include_test_example_bench)
}

pub(crate) fn rvs_load_cargo_project_model_BIS(path: &Path) -> Result<CargoProjectModel, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    rvs_parse_cargo_project_model(&content)
        .map_err(|e| format!("{}: invalid TOML: {e}", cargo_toml.display()))
}

pub(crate) fn rvs_collect_local_crate_prefixes_from_model_BIS(
    path: &Path,
    model: &CargoProjectModel,
    include_test_example_bench: bool,
) -> Result<BTreeSet<CrateName>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let mut prefixes =
        rvs_collect_manifest_crate_prefixes(&model.document, include_test_example_bench)
            .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    let auto_target_flags = rvs_parse_auto_target_flags(&model.document);
    rvs_collect_auto_target_prefixes_for_targets_BIMS(
        path,
        &mut prefixes,
        &include_test_example_bench,
        &auto_target_flags,
    )?;
    Ok(prefixes)
}

pub(crate) fn rvs_detect_local_crate_prefixes_for_function_query_BIS(
    path: &Path,
) -> Result<Option<BTreeSet<CrateName>>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read '{}': {e}", cargo_toml.display()))?;
    let model = rvs_parse_cargo_project_model(&content)
        .map_err(|e| format!("invalid TOML in '{}': {e}", cargo_toml.display()))?;
    if model.document.get("package").is_none() {
        return Ok(None);
    }
    rvs_collect_local_crate_prefixes_from_model_BIS(path, &model, true).map(Some)
}

#[cfg(test)]
pub(crate) fn rvs_collect_auto_target_prefixes_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let model = rvs_parse_cargo_project_model(&content)
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    let auto_target_flags = rvs_parse_auto_target_flags(&model.document);
    rvs_collect_auto_target_prefixes_for_targets_BIMS(path, prefixes, &true, &auto_target_flags)
}

fn rvs_collect_auto_target_prefixes_for_targets_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
    include_test_example_bench: &bool,
    flags: &AutoTargetFlags,
) -> Result<(), String> {
    if *include_test_example_bench && flags.autotests {
        let tests_dir = path.join("tests");
        rvs_collect_auto_target_names_BIMS(&tests_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autoexamples {
        let examples_dir = path.join("examples");
        rvs_collect_auto_target_names_BIMS(&examples_dir, prefixes)?;
    }
    if *include_test_example_bench && flags.autobenches {
        let benches_dir = path.join("benches");
        rvs_collect_auto_target_names_BIMS(&benches_dir, prefixes)?;
    }
    if !flags.autobins {
        return Ok(());
    }
    let bin_dir = path.join("src/bin");
    rvs_collect_auto_target_names_BIMS(&bin_dir, prefixes)?;
    Ok(())
}

fn rvs_parse_cargo_project_model(source: &str) -> Result<CargoProjectModel, toml_edit::TomlError> {
    let document = source.parse::<toml_edit::DocumentMut>()?;
    Ok(CargoProjectModel {
        source: source.to_string(),
        document,
    })
}

fn rvs_parse_auto_target_flags(document: &toml_edit::DocumentMut) -> AutoTargetFlags {
    let package = document.get("package");
    AutoTargetFlags {
        autobins: package
            .and_then(|item| item.get("autobins"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
        autotests: package
            .and_then(|item| item.get("autotests"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
        autoexamples: package
            .and_then(|item| item.get("autoexamples"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
        autobenches: package
            .and_then(|item| item.get("autobenches"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
    }
}

fn rvs_collect_auto_target_names_BIMS(
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
        let path = entry.path();
        if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .ok_or_else(|| format!("auto target file has no stem: {}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("auto target file stem is not UTF-8: {}", path.display()))?;
            rvs_insert_manifest_crate_name_M(prefixes, "auto target file stem", stem)?;
        }
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
