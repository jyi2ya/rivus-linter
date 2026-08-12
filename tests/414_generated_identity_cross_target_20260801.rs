#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_20260801_414_macro_generated_production_function_merges_with_unit_test_BIS() {
    let temp_parent = std::env::var_os("TMPDIR").map_or_else(
        || {
            PathBuf::from(
                std::env::var_os("HOME").expect("never: test environment has HOME or TMPDIR"),
            )
            .join("tmp")
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(&temp_parent).unwrap();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("never: test clock is after unix epoch")
        .as_nanos();
    let project = temp_parent.join(format!(
        "rivus-generated-cross-target-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"generated-cross-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/lib.rs"),
        r#"#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![deny(rivus::rvs_untested_good_fn)]

macro_rules! define_generated {
    () => {
        /// Generated production function.
        pub fn rvs_generated() -> u8 {
            7
        }
    };
}

define_generated!();

#[cfg(test)]
mod tests {
    #[test]
    fn test_20260801_generated_function_is_covered() {
        assert_eq!(super::rvs_generated(), 7);
    }
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-rivus"))
        .arg("check")
        .current_dir(&project)
        .env_remove("RUSTC")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RIVUS_ENABLED")
        .env_remove("RIVUS_WRAPPER")
        .env_remove("RIVUS_GENERATION_ID")
        .env_remove("RIVUS_GENERATION_ROOT")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = format!(
        "success={}\nuntested_generated={}\nmerge_conflict={}\n",
        output.status.success(),
        stderr.contains("good fn 'rvs_generated' not called by any test"),
        stderr.contains("callgraph target record conflict"),
    );
    std::fs::remove_dir_all(&project).unwrap();

    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_out/test_20260801_414_macro_generated_production_function_merges_with_unit_test_BIS.out");
    let expected = std::fs::read_to_string(snapshot).unwrap();
    assert_eq!(summary, expected, "stderr:\n{stderr}");
}
