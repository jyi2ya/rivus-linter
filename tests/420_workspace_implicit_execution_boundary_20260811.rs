#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_20260811_workspace_implicit_execution_does_not_lint_dependency_BIS() {
    let temp_parent = std::env::var_os("TMPDIR").map_or_else(
        || {
            PathBuf::from(
                std::env::var_os("HOME").expect("never: test environment has HOME or TMPDIR"),
            )
            .join("tmp")
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(&temp_parent)
        .expect("never: integration test temporary parent should be created");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("never: test clock is after unix epoch")
        .as_nanos();
    let workspace = temp_parent.join(format!(
        "rivus-workspace-implicit-execution-{}-{unique}",
        std::process::id()
    ));
    for source_dir in [workspace.join("app/src"), workspace.join("fixture-dep/src")] {
        std::fs::create_dir_all(source_dir)
            .expect("never: integration workspace source directory should be created");
    }
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"app\"]\nexclude = [\"fixture-dep\"]\n",
    )
    .expect("never: integration workspace manifest should be written");
    std::fs::write(
        workspace.join("fixture-dep/Cargo.toml"),
        "[package]\nname = \"fixture-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("never: dependency manifest should be written");
    std::fs::write(
        workspace.join("fixture-dep/src/lib.rs"),
        r#"use core::ops::Add;

#[derive(Clone, Copy)]
pub struct Number(pub u8);

impl Add for Number {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

pub fn dependency_uses_overloaded_operator() {
    let _ = Number(1) + Number(2);
}
"#,
    )
    .expect("never: dependency source should be written");
    std::fs::write(
        workspace.join("app/Cargo.toml"),
        r#"[package]
name = "implicit-execution-app"
version = "0.1.0"
edition = "2024"

[dependencies]
fixture-dep = { path = "../fixture-dep" }
"#,
    )
    .expect("never: app manifest should be written");
    std::fs::write(
        workspace.join("app/src/lib.rs"),
        r#"#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

/// Keeps the dependency in the build without using implicit execution locally.
pub fn rvs_workspace_plain() {
    let _ = 1;
}
"#,
    )
    .expect("never: app source should be written");

    let dependency_only_output = Command::new(env!("CARGO_BIN_EXE_cargo-rivus"))
        .arg("check")
        .current_dir(workspace.join("app"))
        .env_remove("RUSTC")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RIVUS_ENABLED")
        .env_remove("RIVUS_WRAPPER")
        .env_remove("RIVUS_GENERATION_ID")
        .env_remove("RIVUS_GENERATION_ROOT")
        .output()
        .expect("never: cargo-rivus integration command should run");
    let dependency_only_stderr = String::from_utf8_lossy(&dependency_only_output.stderr);

    std::fs::write(
        workspace.join("app/src/lib.rs"),
        r#"#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use core::ops::Add;

#[derive(Clone, Copy)]
struct Number(u8);

impl Add for Number {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

/// Exercises an overloaded operator in workspace source.
pub fn rvs_workspace_operator() {
    let _ = Number(1) + Number(2);
}
"#,
    )
    .expect("never: local diagnostic source should be written");
    let local_output = Command::new(env!("CARGO_BIN_EXE_cargo-rivus"))
        .arg("check")
        .current_dir(workspace.join("app"))
        .env_remove("RUSTC")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RIVUS_ENABLED")
        .env_remove("RIVUS_WRAPPER")
        .env_remove("RIVUS_GENERATION_ID")
        .env_remove("RIVUS_GENERATION_ROOT")
        .output()
        .expect("never: cargo-rivus local diagnostic command should run");
    let local_stderr = String::from_utf8_lossy(&local_output.stderr);
    let summary = format!(
        "dependency_only_success={}\nlocal_rejected={}\nlocal_diagnostic={}\ndependency_diagnostic={}\n",
        dependency_only_output.status.success(),
        !local_output.status.success(),
        local_stderr.contains("custom operator or indexing trait implementation"),
        dependency_only_stderr.contains("fixture-dep/src/lib.rs")
            || local_stderr.contains("fixture-dep/src/lib.rs"),
    );
    std::fs::remove_dir_all(&workspace)
        .expect("never: integration workspace cleanup should succeed");

    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "test_out/test_20260811_workspace_implicit_execution_does_not_lint_dependency_BIS.out",
    );
    let expected = std::fs::read_to_string(snapshot)
        .expect("never: workspace implicit execution snapshot should be readable");
    assert_eq!(
        summary, expected,
        "dependency-only stderr:\n{dependency_only_stderr}\nlocal stderr:\n{local_stderr}"
    );
}
