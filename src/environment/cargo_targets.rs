use std::collections::BTreeSet;
use std::path::Path;

use crate::symbols::CrateName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoTargetScope {
    Production,
    WithTestExampleBench,
}

impl CargoTargetScope {
    fn rvs_includes_test_example_bench(self) -> bool {
        matches!(self, Self::WithTestExampleBench)
    }

    pub(crate) fn rvs_cargo_check_arg(self) -> Option<&'static str> {
        match self {
            Self::Production => None,
            Self::WithTestExampleBench => Some("--all-targets"),
        }
    }
}

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

#[cfg(test)]
pub(crate) fn rvs_collect_local_crate_prefixes(toml: &str) -> Result<BTreeSet<CrateName>, String> {
    rvs_collect_local_crate_prefixes_for_targets(toml, CargoTargetScope::WithTestExampleBench)
}

#[cfg(test)]
pub(crate) fn rvs_collect_local_crate_prefixes_for_targets(
    toml: &str,
    scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    let model = rvs_parse_cargo_project_model(toml).map_err(|e| format!("invalid TOML: {e}"))?;
    rvs_collect_manifest_crate_prefixes(&model.document, scope)
}

fn rvs_collect_manifest_crate_prefixes(
    document: &toml_edit::DocumentMut,
    scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    let mut prefixes = BTreeSet::new();
    for (table_name, label) in [("package", "[package].name"), ("lib", "[lib].name")] {
        if let Some(name) = document
            .get(table_name)
            .and_then(|table| table.get("name"))
            .and_then(toml_edit::Item::as_str)
        {
            rvs_insert_manifest_crate_name_M(&mut prefixes, label, name)?;
        }
    }

    for table_name in ["bin", "test", "example", "bench"] {
        if table_name != "bin" && !scope.rvs_includes_test_example_bench() {
            continue;
        }
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
    scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    let model = rvs_load_cargo_project_model_BIS(path)?;
    rvs_collect_local_crate_prefixes_from_model_BIS(path, &model, scope)
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
    scope: CargoTargetScope,
) -> Result<BTreeSet<CrateName>, String> {
    let cargo_toml = path.join("Cargo.toml");
    // Build scripts are compile-time machinery: their crates are excluded from
    // the callgraph, so `build_script_build` is deliberately not a local prefix.
    let prefixes = rvs_collect_manifest_crate_prefixes(&model.document, scope)
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;
    let auto_target_flags = rvs_parse_auto_target_flags(&model.document);
    let mut prefixes = prefixes;
    rvs_collect_auto_target_prefixes_for_targets_BIMS(
        path,
        &mut prefixes,
        &scope,
        &auto_target_flags,
    )?;
    Ok(prefixes)
}

pub(crate) fn rvs_detect_local_crate_prefixes_for_function_query_BIS(
    path: &Path,
    target_scope: CargoTargetScope,
) -> Result<Option<BTreeSet<CrateName>>, String> {
    let model = rvs_load_cargo_project_model_BIS(path)?;
    if model.document.get("package").is_none() {
        return Ok(None);
    }
    rvs_collect_local_crate_prefixes_from_model_BIS(path, &model, target_scope).map(Some)
}

#[cfg(test)]
pub(crate) fn rvs_collect_auto_target_prefixes_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
) -> Result<(), String> {
    let model = rvs_load_cargo_project_model_BIS(path)?;
    let auto_target_flags = rvs_parse_auto_target_flags(&model.document);
    rvs_collect_auto_target_prefixes_for_targets_BIMS(
        path,
        prefixes,
        &CargoTargetScope::WithTestExampleBench,
        &auto_target_flags,
    )
}

fn rvs_collect_auto_target_prefixes_for_targets_BIMS(
    path: &Path,
    prefixes: &mut BTreeSet<CrateName>,
    scope: &CargoTargetScope,
    flags: &AutoTargetFlags,
) -> Result<(), String> {
    let include_optional = scope.rvs_includes_test_example_bench();
    for (enabled, relative_dir) in [
        (include_optional && flags.autotests, "tests"),
        (include_optional && flags.autoexamples, "examples"),
        (include_optional && flags.autobenches, "benches"),
        (flags.autobins, "src/bin"),
    ] {
        if enabled {
            rvs_collect_auto_target_names_BIMS(&path.join(relative_dir), prefixes)?;
        }
    }
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
    let rvs_flag = |name| {
        package
            .and_then(|item| item.get(name))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true)
    };
    AutoTargetFlags {
        autobins: rvs_flag("autobins"),
        autotests: rvs_flag("autotests"),
        autoexamples: rvs_flag("autoexamples"),
        autobenches: rvs_flag("autobenches"),
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
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("readdir error in {}: {e}", dir.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
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
