use std::collections::BTreeSet;
use std::path::Path;

use cargo_metadata::{Target as CargoMetadataTarget, TargetKind};

use crate::symbols::CrateName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoTargetScope {
    Production,
    WithTestExampleBench,
}

impl CargoTargetScope {
    const fn rvs_includes_test_example_bench(self) -> bool {
        matches!(self, Self::WithTestExampleBench)
    }

    pub(crate) const fn rvs_cargo_check_arg(self) -> Option<&'static str> {
        match self {
            Self::Production => None,
            Self::WithTestExampleBench => Some("--all-targets"),
        }
    }
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

/// The primary package of `path` as cargo itself resolves it, or `None`
/// when the manifest defines no package at that path (for example a
/// virtual workspace root).
///
/// One `cargo metadata --no-deps` invocation is the single source of
/// truth for target discovery: explicit manifest tables and cargo's
/// auto-discovery rules alike. The distributed approach cannot drift from
/// cargo's target semantics.
pub(crate) fn rvs_cargo_metadata_primary_package_BIS(
    path: &Path,
    label: &str,
) -> Result<Option<(String, Vec<CargoMetadataTarget>)>, String> {
    let cargo_toml = path.join("Cargo.toml");
    let canonical_manifest = cargo_toml.canonicalize().map_err(|error| {
        format!(
            "cannot canonicalize {label} manifest {}: {error}",
            cargo_toml.display()
        )
    })?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .cargo_path(super::workspace::rvs_cargo_command_from_env_BS())
        .current_dir(path)
        .manifest_path(&canonical_manifest)
        .no_deps()
        .exec()
        .map_err(|error| format!("cargo metadata for {label} failed: {error}"))?;
    let mut selected = None;
    for package in metadata.packages {
        let package_manifest =
            package
                .manifest_path
                .as_std_path()
                .canonicalize()
                .map_err(|error| {
                    format!(
                        "cannot canonicalize cargo metadata manifest {}: {error}",
                        package.manifest_path
                    )
                })?;
        if package_manifest == canonical_manifest && selected.replace(package).is_some() {
            return Err(format!(
                "cargo metadata contains duplicate package records for {}",
                canonical_manifest.display()
            ));
        }
    }
    Ok(selected.map(|package| (package.name.to_string(), package.targets)))
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
    rvs_detect_local_crate_prefixes_opt_BIS(path, scope, "local crate detection")?
        .ok_or_else(rvs_missing_local_crate_target_message)
}

pub(crate) fn rvs_load_cargo_project_model_BIS(path: &Path) -> Result<CargoProjectModel, String> {
    let cargo_toml = path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("cannot read {}: {e}", cargo_toml.display()))?;
    let document = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: invalid TOML: {e}", cargo_toml.display()))?;
    Ok(CargoProjectModel {
        source: content,
        document,
    })
}

pub(crate) fn rvs_detect_local_crate_prefixes_for_function_query_BIS(
    path: &Path,
    target_scope: CargoTargetScope,
) -> Result<Option<BTreeSet<CrateName>>, String> {
    rvs_detect_local_crate_prefixes_opt_BIS(path, target_scope, "function query")
}

fn rvs_missing_local_crate_target_message() -> String {
    "Cargo.toml: missing local crate target ([package].name, [lib].name, [[bin]].name, [[test]].name, [[example]].name, or [[bench]].name)".to_string()
}

fn rvs_detect_local_crate_prefixes_opt_BIS(
    path: &Path,
    scope: CargoTargetScope,
    label: &str,
) -> Result<Option<BTreeSet<CrateName>>, String> {
    let Some((package_name, targets)) = rvs_cargo_metadata_primary_package_BIS(path, label)? else {
        return Ok(None);
    };
    let mut prefixes = BTreeSet::new();
    // The package name is always a local prefix candidate: bin-only
    // packages name their crate after the package, and a package without
    // any selected target still keeps its package name as a prefix.
    rvs_insert_manifest_crate_name_M(&mut prefixes, "[package].name", &package_name)?;
    for target in &targets {
        // Build scripts are compile-time machinery: their crates are
        // excluded from the callgraph, so `build_script_build` is
        // deliberately not a local prefix.
        if target.kind.contains(&TargetKind::CustomBuild) {
            continue;
        }
        let scope_excluded = !scope.rvs_includes_test_example_bench()
            && target.kind.iter().any(|kind| {
                matches!(
                    kind,
                    TargetKind::Test | TargetKind::Example | TargetKind::Bench
                )
            });
        if scope_excluded {
            continue;
        }
        rvs_insert_manifest_crate_name_M(
            &mut prefixes,
            "cargo metadata target name",
            &target.name,
        )?;
    }
    Ok(Some(prefixes))
}
