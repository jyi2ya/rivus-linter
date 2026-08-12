#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_20260801_418_macro_repetition_definitions_have_distinct_stable_identities_BIS() {
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
    let project = temp_parent.join(format!(
        "rivus-macro-repetition-identity-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(project.join("src"))
        .expect("never: macro repetition project source directory should be created");
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"macro-repetition-identity\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("never: macro repetition manifest should be written");
    std::fs::write(
        project.join("src/lib.rs"),
        r#"#![feature(register_tool)]
#![register_tool(rivus)]
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

macro_rules! invoke_generated {
    ($($callee:path),+ $(,)?) => {
        $({
            fn rvs_generated_S() {
                $callee();
            }
            rvs_generated_S();
        })+
    };
}

pub fn rvs_invoke_both_S() {
    invoke_generated!(rvs_first_S, rvs_second_S);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_20260801_macro_repetition_invokes_both_definitions() {
        super::rvs_invoke_both_S();
    }
}
"#,
    )
    .expect("never: macro repetition source should be written");

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
        .expect("never: cargo-rivus integration command should run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = format!(
        "success={}\ntarget_record_conflict={}\nidentity_marker_leaked={}\n",
        output.status.success(),
        stderr.contains("callgraph target record conflict"),
        stderr.contains("{impl#") || stderr.contains("{def#"),
    );
    std::fs::remove_dir_all(&project)
        .expect("never: macro repetition project cleanup should succeed");

    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "test_out/test_20260801_418_macro_repetition_definitions_have_distinct_stable_identities_BIS.out",
    );
    let expected = std::fs::read_to_string(snapshot)
        .expect("never: macro repetition identity snapshot should be readable");
    assert_eq!(summary, expected, "stderr:\n{stderr}");
}
