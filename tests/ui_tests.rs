use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn driver_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().and_then(|p| p.parent()).unwrap();
    dir.join("cargo-rivus")
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
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

fn normalize_stderr(raw: &str) -> String {
    let dir = std::env::current_dir().unwrap();
    let dir_str = dir.to_string_lossy().to_string();
    let mut out = raw.to_string();
    out = out.replace(&dir_str, "$DIR");
    for cap in ['A', 'B', 'I', 'M', 'S', 'T', 'U'] {
        out = out.replace(&format!("rivus::rvs_{cap}"), &format!("rivus::rvs_{cap}"));
    }
    let lines: Vec<&str> = out.lines().filter(|l| !l.contains("generated")).collect();
    lines.join("\n").trim_end().to_string()
}

fn run_one_test(fixture: &Path, stderr_path: &Path, bless: bool) -> Result<(), String> {
    let driver = driver_path();
    if !driver.exists() {
        return Err(format!("cargo-rivus not found at {:?}", driver));
    }

    // Locate caps/ directory (next to the driver binary's source tree)
    let caps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("caps");

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

    let mut cmd = Command::new(&driver);
    cmd.env("RIVUS_ENABLED", "1")
        .env("RIVUS_CAPSMAP", &caps_dir)
        .arg("rustc")
        .arg("--edition=2024")
        .arg("--emit=metadata")
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
    let actual = normalize_stderr(&raw_stderr);

    if check_pass {
        if !actual.is_empty() {
            return Err(format!(
                "{:?}: check-pass but got output:\n{}",
                fixture.file_name().unwrap(),
                actual
            ));
        }
        if bless && stderr_path.exists() {
            let _ = fs::remove_file(stderr_path);
        }
        return Ok(());
    }

    if bless {
        if actual.is_empty() {
            let _ = fs::remove_file(stderr_path);
        } else {
            fs::write(stderr_path, actual + "\n").map_err(|e| format!("write: {e}"))?;
        }
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
fn ui_tests() {
    let bless = std::env::var("RUSTC_BLESS").is_ok() || std::env::args().any(|a| a == "--bless");
    let filter = std::env::args()
        .find(|a| !a.starts_with('-') && a != "ui_tests" && !a.contains("ui_tests"));
    let ui_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let fixtures = collect_rs_files(&ui_dir);
    assert!(!fixtures.is_empty(), "no .rs fixtures in tests/ui/");

    let mut failures = Vec::new();
    for fixture in &fixtures {
        let name = fixture.file_stem().unwrap().to_string_lossy().to_string();
        if let Some(ref f) = filter {
            if !name.contains(f.as_str()) {
                continue;
            }
        }
        let stderr_path = fixture.with_extension("stderr");
        if let Err(e) = run_one_test(fixture, &stderr_path, bless) {
            failures.push((name, e));
        }
    }

    if !failures.is_empty() {
        for (name, err) in &failures {
            eprintln!("FAIL {name}: {err}");
        }
        panic!("{} UI test(s) failed", failures.len());
    }
}
