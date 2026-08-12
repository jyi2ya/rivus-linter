#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_20260801_417_impl_marker_recurses_into_same_name_dependency_versions_BIS() {
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
        "rivus-impl-nested-crate-identity-{}-{unique}",
        std::process::id()
    ));
    for source_dir in [
        workspace.join("app/src"),
        workspace.join("marker-v1/src"),
        workspace.join("marker-v2/src"),
    ] {
        std::fs::create_dir_all(source_dir)
            .expect("never: integration workspace source directory should be created");
    }
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"app\"]\nexclude = [\"marker-v1\", \"marker-v2\"]\n",
    )
    .expect("never: integration workspace manifest should be written");
    std::fs::write(
        workspace.join("marker-v1/Cargo.toml"),
        "[package]\nname = \"marker\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    )
    .expect("never: marker v1 manifest should be written");
    std::fs::write(
        workspace.join("marker-v1/src/lib.rs"),
        "pub trait Bound {}\n\npub struct Marker;\n\nimpl Bound for Marker {}\n",
    )
    .expect("never: marker v1 source should be written");
    std::fs::write(
        workspace.join("marker-v2/Cargo.toml"),
        r#"[package]
name = "marker"
version = "2.0.0"
edition = "2024"

[dependencies]
marker_v1 = { package = "marker", path = "../marker-v1", version = "1" }
"#,
    )
    .expect("never: marker v2 manifest should be written");
    std::fs::write(
        workspace.join("marker-v2/src/lib.rs"),
        r#"pub trait Bound: marker_v1::Bound {}

pub struct Marker;

impl marker_v1::Bound for Marker {}
impl Bound for Marker {}
"#,
    )
    .expect("never: marker v2 source should be written");
    std::fs::write(
        workspace.join("app/Cargo.toml"),
        r#"[package]
name = "nested-crate-identity-app"
version = "0.1.0"
edition = "2024"

[dependencies]
marker_v1 = { package = "marker", path = "../marker-v1", version = "1" }
marker_v2 = { package = "marker", path = "../marker-v2", version = "2" }
"#,
    )
    .expect("never: integration app manifest should be written");
    std::fs::write(
        workspace.join("app/src/lib.rs"),
        r#"#![feature(register_tool)]
#![feature(specialization)]
#![register_tool(rivus)]
#![allow(incomplete_features)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

static FIRST: u8 = 1;
static SECOND: u8 = 2;

fn rvs_first_S() {
    let _ = FIRST;
}

fn rvs_second_S() {
    let _ = SECOND;
}

pub struct Worker<T>(pub T);

impl Worker<marker_v1::Marker> {
    pub fn rvs_run_S(&self) {
        rvs_first_S();
    }
}

impl Worker<marker_v2::Marker> {
    pub fn rvs_run_S(&self) {
        rvs_second_S();
    }
}

trait Specialized {
    fn rvs_specialized_S();
}

impl<T: marker_v1::Bound> Specialized for T {
    default fn rvs_specialized_S() {
        rvs_first_S();
    }
}

impl<T: marker_v2::Bound> Specialized for T {
    fn rvs_specialized_S() {
        rvs_second_S();
    }
}

#[cfg(test)]
mod tests {
    use super::{Specialized, Worker};

    #[test]
    fn test_20260801_both_dependency_versions_keep_distinct_impls() {
        Worker(marker_v1::Marker).rvs_run_S();
        Worker(marker_v2::Marker).rvs_run_S();
        <marker_v1::Marker as Specialized>::rvs_specialized_S();
        <marker_v2::Marker as Specialized>::rvs_specialized_S();
    }
}
"#,
    )
    .expect("never: integration app source should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-rivus"))
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = format!(
        "success={}\ntarget_record_conflict={}\nidentity_marker_leaked={}\n",
        output.status.success(),
        stderr.contains("callgraph target record conflict"),
        stderr.contains("{impl#") || stderr.contains("{def#"),
    );
    std::fs::remove_dir_all(&workspace)
        .expect("never: integration workspace cleanup should succeed");

    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "test_out/test_20260801_417_impl_marker_recurses_into_same_name_dependency_versions_BIS.out",
    );
    let expected = std::fs::read_to_string(snapshot)
        .expect("never: nested crate identity snapshot should be readable");
    assert_eq!(summary, expected, "stderr:\n{stderr}");
}
