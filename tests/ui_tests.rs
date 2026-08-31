#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]
#![allow(
    rivus::rvs_unsupported_implicit_execution,
    reason = "UI generation directories must be removed when a fixture panics"
)]

use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

const RVS_UI_GENERATION_MARKER_FILE: &str = ".rivus-generation.json";
const RVS_RUN_GENERATION_SCHEMA_VERSION: u32 = 5;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum UiGenerationTargetScope {
    WithTestExampleBench,
}

#[derive(Debug, Serialize)]
#[serde(tag = "input", rename_all = "snake_case")]
enum UiGenerationAnalysisMode {
    ProjectCaps,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UiGenerationMode {
    Analysis {
        target_scope: UiGenerationTargetScope,
        analysis: UiGenerationAnalysisMode,
    },
}

#[derive(Debug, Serialize)]
struct UiGenerationMarker {
    schema_version: u32,
    generation_id: String,
    project_root: PathBuf,
    mode: UiGenerationMode,
}

#[derive(Debug)]
struct UiDriverGeneration {
    temp_dir: Option<tempfile::TempDir>,
    root: PathBuf,
    generation_id: String,
}

impl UiDriverGeneration {
    fn rvs_new_BIST(project: &Path) -> Result<Self, String> {
        let project = project
            .canonicalize()
            .map_err(|error| format!("cannot canonicalize UI project: {error}"))?;
        let lexical_runs_dir = project.join("target/.rivus-runs");
        fs::create_dir_all(&lexical_runs_dir)
            .map_err(|error| format!("cannot create UI generation directory: {error}"))?;
        let runs_dir = lexical_runs_dir
            .canonicalize()
            .map_err(|error| format!("cannot canonicalize UI generation directory: {error}"))?;
        let temp_dir = tempfile::Builder::new()
            .prefix("rivus-v4-analysis-all-targets-")
            .tempdir_in(&runs_dir)
            .map_err(|error| format!("cannot reserve UI generation: {error}"))?;
        let root = temp_dir.path().to_path_buf();
        let generation_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "UI generation directory name is not UTF-8".to_string())?
            .to_string();
        let artifact_dir = root.join("artifacts");
        fs::create_dir(&artifact_dir)
            .map_err(|error| format!("cannot create UI generation artifacts: {error}"))?;
        let marker = UiGenerationMarker {
            schema_version: RVS_RUN_GENERATION_SCHEMA_VERSION,
            generation_id: generation_id.clone(),
            project_root: project,
            mode: UiGenerationMode::Analysis {
                target_scope: UiGenerationTargetScope::WithTestExampleBench,
                analysis: UiGenerationAnalysisMode::ProjectCaps,
            },
        };
        let marker_json = serde_json::to_vec(&marker)
            .map_err(|error| format!("cannot serialize UI generation marker: {error}"))?;
        let ready_path = root.join(RVS_UI_GENERATION_MARKER_FILE);
        let mut ready = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ready_path)
            .map_err(|error| format!("cannot create UI generation marker: {error}"))?;
        ready
            .write_all(&marker_json)
            .map_err(|error| format!("cannot write UI generation marker: {error}"))?;
        ready
            .sync_all()
            .map_err(|error| format!("cannot sync UI generation marker: {error}"))?;
        Ok(Self {
            temp_dir: Some(temp_dir),
            root,
            generation_id,
        })
    }
}

impl Drop for UiDriverGeneration {
    fn drop(&mut self) {
        if let Some(temp_dir) = self.temp_dir.take() {
            if let Err(error) = temp_dir.close() {
                eprintln!(
                    "warning: cannot remove UI generation {}: {error}",
                    self.root.display()
                );
            }
        }
    }
}

fn rvs_driver_path_BIS() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().and_then(|p| p.parent()).unwrap();
    dir.join("cargo-rivus")
}

fn rvs_collect_rs_files_BIS(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "rs") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

fn rvs_normalize_stderr_BIS(raw: &str) -> String {
    let dir = std::env::current_dir().unwrap();
    let dir_str = dir.to_string_lossy().to_string();
    let mut out = raw.to_string();
    out = out.replace(&dir_str, "$DIR");
    out.trim_end().to_string()
}

fn rvs_snapshot_mode_error(
    check_pass: bool,
    bless: bool,
    snapshot_exists: bool,
    snapshot_has_content: bool,
) -> Option<&'static str> {
    match (check_pass, bless, snapshot_exists, snapshot_has_content) {
        (true, false, true, _) => {
            Some("check-pass fixture has a stale .stderr snapshot; remove it or bless tests")
        }
        (false, false, false, _) => {
            Some("non-check-pass fixture is missing its required .stderr snapshot")
        }
        (false, false, true, false) => Some("non-check-pass fixture has an empty .stderr snapshot"),
        _ => None,
    }
}

fn rvs_non_check_pass_output_error(
    status_success: bool,
    diagnostic_has_content: bool,
) -> Option<&'static str> {
    match (status_success, diagnostic_has_content) {
        (true, _) => Some("non-check-pass fixture compiled successfully"),
        (false, false) => Some("compiler failed with no diagnostic output"),
        (false, true) => None,
    }
}

fn rvs_ui_filter_BS() -> Result<Option<String>, String> {
    let Some(filter) = std::env::var_os("RIVUS_UI_FILTER") else {
        return Ok(None);
    };
    let filter = filter
        .into_string()
        .map_err(|_| "RIVUS_UI_FILTER must be valid UTF-8".to_string())?;
    if filter.is_empty() {
        return Err("RIVUS_UI_FILTER must not be empty".to_string());
    }
    Ok(Some(filter))
}

fn rvs_bless_value_enabled(value: Option<&OsStr>) -> bool {
    value.and_then(OsStr::to_str) == Some("1")
}

fn rvs_bless_enabled_BS() -> bool {
    rvs_bless_value_enabled(std::env::var_os("RUSTC_BLESS").as_deref())
}

#[test]
fn test_20260716_ui_bless_requires_rustc_bless_one() {
    let cases = [
        ("missing", None, false),
        ("empty", Some(OsStr::new("")), false),
        ("zero", Some(OsStr::new("0")), false),
        ("one", Some(OsStr::new("1")), true),
        ("true", Some(OsStr::new("true")), false),
    ];
    let mut output = String::new();
    for (label, value, expected) in cases {
        let actual = rvs_bless_value_enabled(value);
        output.push_str(&format!("{label}={actual}\n"));
        assert_eq!(actual, expected);
    }
    let snapshot = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_out/test_20260716_ui_bless_requires_rustc_bless_one.out"),
    )
    .unwrap();
    assert_eq!(output, snapshot);
}

#[cfg(unix)]
#[test]
fn test_20260731_ui_generation_canonicalizes_symlinked_target() {
    use std::os::unix::fs::symlink;

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("never: UI test clock should follow the unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rivus-ui-canonical-target-{}-{unique}",
        std::process::id()
    ));
    let project = dir.join("project");
    let physical_target = dir.join("physical-target");
    fs::create_dir_all(&project).expect("never: UI test project should be created");
    fs::create_dir(&physical_target).expect("never: UI physical target should be created");
    symlink(&physical_target, project.join("target"))
        .expect("never: UI target symlink should be created");

    let generation = UiDriverGeneration::rvs_new_BIST(&project)
        .expect("never: UI generation under a symlinked target should be reserved");
    let root_is_canonical = generation.root.canonicalize().is_ok_and(|canonical| {
        canonical == generation.root && canonical.starts_with(&physical_target)
    });
    let output = format!("root_is_canonical={root_is_canonical}\n");
    let snapshot = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_out/test_20260731_ui_generation_canonicalizes_symlinked_target.out"),
    )
    .expect("never: UI canonical-target snapshot should be readable");
    assert_eq!(output, snapshot);
    assert!(root_is_canonical);

    drop(generation);
    fs::remove_dir_all(dir).expect("never: UI canonical-target fixture cleanup should succeed");
}

fn rvs_run_one_test_BIS(
    fixture: &Path,
    stderr_path: &Path,
    generation: &UiDriverGeneration,
) -> Result<(), String> {
    let bless = rvs_bless_enabled_BS();
    let driver = rvs_driver_path_BIS();
    if !driver.exists() {
        return Err(format!("cargo-rivus not found at {:?}", driver));
    }

    let caps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("caps");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("rivus-ui-tests");
    fs::create_dir_all(&out_dir).map_err(|e| format!("create {:?}: {e}", out_dir))?;

    let source = fs::read_to_string(fixture).map_err(|e| format!("read {:?}: {e}", fixture))?;
    let mut extra_args: Vec<String> = Vec::new();
    let mut use_test_crate = false;
    let mut check_pass = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(comment) = trimmed.strip_prefix("//") else {
            break;
        };
        let directive = comment.trim();
        if directive == "check-pass" {
            check_pass = true;
        }
        if let Some(rest) = directive.strip_prefix("compile-flags:") {
            for arg in rest.split_whitespace() {
                if arg == "--test" {
                    use_test_crate = true;
                } else {
                    extra_args.push(arg.to_string());
                }
            }
        }
    }
    let snapshot_exists = stderr_path.exists();
    let snapshot_has_content = if snapshot_exists {
        !fs::read_to_string(stderr_path)
            .map_err(|error| format!("read {stderr_path:?}: {error}"))?
            .trim()
            .is_empty()
    } else {
        false
    };
    if let Some(error) =
        rvs_snapshot_mode_error(check_pass, bless, snapshot_exists, snapshot_has_content)
    {
        return Err(error.to_string());
    }

    let mut cmd = Command::new(&driver);
    for key in [
        "RIVUS_CALLGRAPH",
        "RIVUS_CALLGRAPH_DIR",
        "RIVUS_CRATE_PROVENANCE",
        "RIVUS_CAPSMAP",
        "RIVUS_OFFLINE_EMISSIONS",
        "RIVUS_OFFLINE_EMISSIONS_ACK_DIR",
        "RIVUS_OFFLINE_CAPS",
        "RIVUS_UI_TESTING",
        "RIVUS_UNTESTED_PATHS",
        "RIVUS_ENABLED",
        "RIVUS_WRAPPER",
        "RIVUS_GENERATION_ID",
        "RIVUS_GENERATION_ROOT",
        "CARGO_PRIMARY_PACKAGE",
    ] {
        cmd.env_remove(key);
    }
    cmd.env("RIVUS_ENABLED", "1")
        .env("RIVUS_WRAPPER", "1")
        .env("RIVUS_GENERATION_ID", &generation.generation_id)
        .env("RIVUS_GENERATION_ROOT", &generation.root)
        .env("RIVUS_CAPSMAP", &caps_dir)
        .env("RIVUS_UI_TESTING", "1")
        .arg("rustc")
        .arg("--edition=2024")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("-Aunused")
        .arg("-Ainternal_features")
        .arg("-Zui-testing")
        .arg("-Zdeduplicate-diagnostics=no")
        .arg("-Dwarnings");

    if use_test_crate {
        cmd.arg("--test");
    } else {
        cmd.arg("--crate-type=lib");
    }

    let output = cmd
        .args(&extra_args)
        .arg(fixture)
        .current_dir(fixture.parent().unwrap())
        .output()
        .map_err(|e| format!("failed to run rivus-driver: {e}"))?;

    let raw_stderr = String::from_utf8_lossy(&output.stderr);
    let actual = rvs_normalize_stderr_BIS(&raw_stderr);

    if check_pass {
        if !output.status.success() || !actual.is_empty() {
            return Err(format!(
                "{:?}: check-pass failed with status {}:\n{}",
                fixture.file_name().unwrap(),
                output.status,
                actual
            ));
        }
        if bless && stderr_path.exists() {
            fs::remove_file(stderr_path).map_err(|e| format!("remove {:?}: {e}", stderr_path))?;
        }
        return Ok(());
    }

    if bless {
        if let Some(error) =
            rvs_non_check_pass_output_error(output.status.success(), !actual.is_empty())
        {
            return Err(format!(
                "{:?}: {error}; status {}. Add // check-pass if successful compilation is intentional",
                fixture.file_name().unwrap(),
                output.status,
            ));
        }
        fs::write(stderr_path, actual + "\n").map_err(|e| format!("write: {e}"))?;
        return Ok(());
    }

    let expected = if stderr_path.exists() {
        fs::read_to_string(stderr_path)
            .map_err(|e| format!("read {:?}: {e}", stderr_path))?
            .trim_end()
            .to_string()
    } else {
        String::new()
    };

    let actual_trimmed = actual.trim_end().to_string();
    if let Some(error) =
        rvs_non_check_pass_output_error(output.status.success(), !actual_trimmed.is_empty())
    {
        return Err(format!(
            "{:?}: {error}; status {}",
            fixture.file_name().unwrap(),
            output.status
        ));
    }
    if actual_trimmed != expected {
        Err(format!(
            "stderr mismatch for {:?}\n\n--- expected ---\n{}\n\n--- actual ---\n{}\n",
            fixture.file_name().unwrap(),
            expected,
            actual_trimmed
        ))
    } else {
        Ok(())
    }
}

#[test]
fn test_20260630_ui_tests_BIS() -> Result<(), String> {
    assert!(rvs_snapshot_mode_error(true, false, true, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, false, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, true, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, true, true).is_none());
    assert!(rvs_non_check_pass_output_error(true, true).is_some());
    assert!(rvs_non_check_pass_output_error(false, false).is_some());
    assert!(rvs_non_check_pass_output_error(false, true).is_none());

    let filter = rvs_ui_filter_BS()?;
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generation = UiDriverGeneration::rvs_new_BIST(&project)?;
    let ui_dir = project.join("tests/ui");
    let fixtures = rvs_collect_rs_files_BIS(&ui_dir);
    assert!(!fixtures.is_empty(), "no .rs fixtures in tests/ui/");
    let mut orphan_snapshots = fs::read_dir(&ui_dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "stderr")
        })
        .filter(|path| !path.with_extension("rs").exists())
        .collect::<Vec<_>>();
    orphan_snapshots.sort();
    assert!(
        orphan_snapshots.is_empty(),
        "orphan UI snapshots without fixtures: {orphan_snapshots:?}"
    );

    let mut failures = Vec::new();
    let mut selected_count = 0usize;
    for fixture in &fixtures {
        let name = fixture.file_stem().unwrap().to_string_lossy().to_string();
        if let Some(ref f) = filter
            && !name.contains(f.as_str())
        {
            continue;
        }
        selected_count += 1;
        let stderr_path = fixture.with_extension("stderr");
        if let Err(e) = rvs_run_one_test_BIS(fixture, &stderr_path, &generation) {
            failures.push((name, e));
        }
    }
    assert_ne!(
        selected_count, 0,
        "RIVUS_UI_FILTER {:?} selected no UI fixtures",
        filter
    );

    if !failures.is_empty() {
        for (name, err) in &failures {
            eprintln!("FAIL {name}: {err}");
        }
        return Err(format!("{} UI test(s) failed", failures.len()));
    }
    Ok(())
}
