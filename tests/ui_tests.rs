#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn rvs_normalize_stderr_S(raw: &str) -> String {
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

fn rvs_run_one_test_BIS(fixture: &Path, stderr_path: &Path) -> Result<(), String> {
    let bless = std::env::var_os("RUSTC_BLESS")
        .as_deref()
        .and_then(OsStr::to_str)
        == Some("1")
        || std::env::args().any(|argument| argument == "--bless");
    let driver = rvs_driver_path_BIS();
    if !driver.exists() {
        return Err(format!("cargo-rivus not found at {:?}", driver));
    }

    // Locate caps/ directory (next to the driver binary's source tree)
    let caps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("caps");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("rivus-ui-tests");
    fs::create_dir_all(&out_dir).map_err(|e| format!("create {:?}: {e}", out_dir))?;

    // Parse // compile-flags: and // check-pass directives from the fixture
    let source = fs::read_to_string(fixture).map_err(|e| format!("read {:?}: {e}", fixture))?;
    let mut extra_args: Vec<String> = Vec::new();
    let mut use_test_crate = false;
    let mut check_pass = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "// check-pass" {
            check_pass = true;
        }
        if let Some(rest) = trimmed.strip_prefix("// compile-flags:") {
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
    cmd.env("RIVUS_ENABLED", "1")
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
    let actual = rvs_normalize_stderr_S(&raw_stderr);

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
fn test_20260630_ui_tests_BIS() {
    assert!(rvs_snapshot_mode_error(true, false, true, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, false, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, true, false).is_some());
    assert!(rvs_snapshot_mode_error(false, false, true, true).is_none());
    assert!(rvs_non_check_pass_output_error(true, true).is_some());
    assert!(rvs_non_check_pass_output_error(false, false).is_some());
    assert!(rvs_non_check_pass_output_error(false, true).is_none());

    let filter = rvs_ui_filter_BS().unwrap_or_else(|error| panic!("{error}"));
    let ui_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
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
        if let Err(e) = rvs_run_one_test_BIS(fixture, &stderr_path) {
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
        panic!("{} UI test(s) failed", failures.len());
    }
}
