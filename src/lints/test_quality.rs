use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use rustc_lint::{LateContext, LintContext};

use super::ctx::{CoverageFn, TestCallTarget};
use super::msg::Msg;
use super::{
    RVS_DUPLICATE_TEST, RVS_MISSING_TEST_OUTPUT, RVS_UNTESTED_GOOD_FN, RVS_UNTESTED_OK_FN,
};
use crate::artifacts::FnGraph;
use crate::fs_guard::rvs_render_atomic_write_failure;
use crate::symbols::CrateName;

/// `check_crate_post` — cross-cutting test quality checks and output writing.
pub(crate) fn rvs_check_crate_post_BIMS<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
    good_fns: &[CoverageFn],
    ok_fns: &[CoverageFn],
    test_calls: &HashSet<TestCallTarget>,
    callgraph: &FnGraph,
    collect_callgraph: bool,
) {
    rvs_check_duplicate_tests_S(cx, test_names);
    rvs_check_missing_test_output_BIS(cx, test_names);
    rvs_check_untested_good_fns_S(cx, good_fns, test_calls);
    rvs_check_untested_ok_fns_S(cx, ok_fns, test_calls);
    rvs_write_callgraph_BIS(cx, callgraph, collect_callgraph);
}

fn rvs_check_duplicate_tests_S<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
) {
    for (name, spans) in test_names {
        if spans.len() > 1 {
            for sp in spans {
                cx.emit_span_lint(
                    RVS_DUPLICATE_TEST,
                    *sp,
                    Msg::rvs_new(*sp, format!("duplicate test '{name}'")),
                );
            }
        }
    }
}

fn rvs_check_missing_test_output_BIS<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
) {
    if rvs_env_os_flag_enabled_BS("RIVUS_UI_TESTING") {
        return;
    }
    let out_dir = Path::new("test_out");
    for (name, spans) in test_names {
        let out_file = format!("test_out/{name}.out");
        if !rvs_has_test_output_BIS(name, out_dir) {
            if let Some(sp) = spans.first() {
                cx.emit_span_lint(
                    RVS_MISSING_TEST_OUTPUT,
                    *sp,
                    Msg::rvs_new(*sp, format!("test '{name}' missing {out_file}")),
                );
            }
        }
    }
}

fn rvs_has_test_output_BIS(name: &str, out_dir: &Path) -> bool {
    out_dir.join(format!("{name}.out")).is_file()
}

fn rvs_check_untested_good_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    good_fns: &[CoverageFn],
    test_calls: &HashSet<TestCallTarget>,
) {
    for candidate in good_fns {
        if !rvs_test_calls_function(test_calls, candidate) {
            cx.emit_span_lint(
                RVS_UNTESTED_GOOD_FN,
                candidate.span,
                Msg::rvs_new(
                    candidate.span,
                    format!("good fn '{}' not called by any test", candidate.name),
                ),
            );
        }
    }
}

fn rvs_check_untested_ok_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    ok_fns: &[CoverageFn],
    test_calls: &HashSet<TestCallTarget>,
) {
    for candidate in ok_fns {
        if !rvs_test_calls_function(test_calls, candidate) {
            cx.emit_span_lint(
                RVS_UNTESTED_OK_FN,
                candidate.span,
                Msg::rvs_new(
                    candidate.span,
                    format!("ok fn '{}' not called by any test", candidate.name),
                ),
            );
        }
    }
}

fn rvs_test_calls_function(test_calls: &HashSet<TestCallTarget>, candidate: &CoverageFn) -> bool {
    test_calls.contains(&TestCallTarget::Resolved(candidate.def_path.clone()))
        || test_calls.contains(&TestCallTarget::UnresolvedName(candidate.name.clone()))
}

fn rvs_env_os_flag_enabled_BS(name: &str) -> bool {
    rvs_env_os_flag_value_enabled(std::env::var_os(name).as_deref())
}

fn rvs_env_os_flag_value_enabled(value: Option<&OsStr>) -> bool {
    value.and_then(OsStr::to_str) == Some("1")
}

fn rvs_write_callgraph_BIS<'tcx>(
    cx: &LateContext<'tcx>,
    callgraph: &FnGraph,
    collect_callgraph: bool,
) {
    if collect_callgraph {
        if !callgraph.rvs_is_empty() {
            let cg_dir = std::env::var_os("RIVUS_CALLGRAPH_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("target/rivus-callgraph"));
            let crate_name = CrateName::rvs_from_manifest_name(
                cx.tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).as_str(),
            );
            if let Err(e) = rvs_write_callgraph_artifact_BIS(&cg_dir, &crate_name, callgraph) {
                cx.tcx
                    .dcx()
                    .err(format!("cannot write rivus callgraph artifact: {e}"));
            }
        }
    }
}

fn rvs_write_callgraph_artifact_BIS(
    artifact_dir: &Path,
    crate_name: &CrateName,
    callgraph: &FnGraph,
) -> Result<PathBuf, String> {
    if callgraph.rvs_is_empty() {
        return Err("callgraph artifact must contain at least one node".into());
    }
    let crate_name_str = crate_name.rvs_as_str();
    if crate_name_str.is_empty()
        || crate_name_str.contains('/')
        || crate_name_str.contains('\\')
        || crate_name_str.contains('\0')
    {
        return Err(format!(
            "artifact crate name must be a non-empty path segment: {crate_name}"
        ));
    }
    let json = crate::artifacts::rvs_serialize_callgraph_json_S(callgraph)?;
    std::fs::create_dir_all(artifact_dir)
        .map_err(|e| format!("cannot create {}: {e}", artifact_dir.display()))?;
    let final_path = artifact_dir.join(format!("{crate_name}-{}.json", std::process::id()));
    crate::fs_guard::rvs_write_atomic_BIS(&final_path, json.as_bytes(), |attempt| {
        if attempt == 0 {
            artifact_dir.join(format!("{crate_name}-{}.json.tmp", std::process::id()))
        } else {
            artifact_dir.join(format!(
                "{crate_name}-{}.json.tmp.{attempt}",
                std::process::id()
            ))
        }
    })
    .map_err(|failure| {
        rvs_render_atomic_write_failure(failure, &final_path, "temp artifact", false)
    })?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FnNode;
    use crate::symbols::DefPath;
    use crate::test_support::rvs_snapshot_BIS;

    fn rvs_test_callgraph() -> FnGraph {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::rvs_run"), FnNode::default());
        graph
    }

    #[test]
    fn test_20260703_has_test_output_false_when_dir_missing() {
        let missing_dir = Path::new("/definitely/not/present/rivus-test-out");
        let exists = rvs_has_test_output_BIS(
            "test_20260703_has_test_output_false_when_dir_missing",
            missing_dir,
        );
        rvs_snapshot_BIS(
            "test_20260703_has_test_output_false_when_dir_missing",
            &format!("exists={exists}\n"),
        );
        assert!(!exists);
    }

    #[test]
    fn test_20260703_has_test_output_true_for_existing_snapshot() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-test-quality-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("test_20260703_has_test_output_true_for_existing_snapshot.out"),
            "ok\n",
        )
        .unwrap();

        let exists = rvs_has_test_output_BIS(
            "test_20260703_has_test_output_true_for_existing_snapshot",
            &dir,
        );
        rvs_snapshot_BIS(
            "test_20260703_has_test_output_true_for_existing_snapshot",
            &format!("exists={exists}\n"),
        );
        assert!(exists);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_has_test_output_false_for_snapshot_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-test-quality-dir-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(
            dir.join("test_20260706_has_test_output_false_for_snapshot_directory.out"),
        )
        .unwrap();

        let exists = rvs_has_test_output_BIS(
            "test_20260706_has_test_output_false_for_snapshot_directory",
            &dir,
        );
        rvs_snapshot_BIS(
            "test_20260706_has_test_output_false_for_snapshot_directory",
            &format!("exists={exists}\n"),
        );
        assert!(!exists);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_env_os_flag_value_requires_one() {
        let cases = [
            (None, false),
            (Some(OsStr::new("")), false),
            (Some(OsStr::new("0")), false),
            (Some(OsStr::new("true")), false),
            (Some(OsStr::new("1")), true),
        ];
        let output = cases
            .iter()
            .map(|(value, _)| format!("{value:?}={}", rvs_env_os_flag_value_enabled(*value)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260706_env_os_flag_value_requires_one", &output);

        for (value, expected) in cases {
            assert_eq!(rvs_env_os_flag_value_enabled(value), expected);
        }
    }

    #[test]
    fn test_20260706_write_json_artifact_uses_final_json_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-write-{}-{unique}",
            std::process::id()
        ));
        let graph = rvs_test_callgraph();
        let path = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("demo"), &graph)
            .expect("artifact write should succeed");
        let tmp_exists = dir
            .join(format!("demo-{}.json.tmp", std::process::id()))
            .exists();
        let file_name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(&std::process::id().to_string(), "$PID");
        let output = format!(
            "file={}\ncontent={}\ntmp_exists={}\n",
            file_name,
            std::fs::read_to_string(&path).unwrap(),
            tmp_exists
        );
        rvs_snapshot_BIS(
            "test_20260706_write_json_artifact_uses_final_json_file",
            &output,
        );

        assert!(path.is_file());
        assert!(!tmp_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260707_write_json_artifact_rejects_empty_json() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-empty-json-{}-{unique}",
            std::process::id()
        ));

        let graph = FnGraph::rvs_new();
        let result = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("demo"), &graph);
        let dir_exists = dir.exists();
        let output = format!("result={result:?}\ndir_exists={dir_exists}\n");
        rvs_snapshot_BIS(
            "test_20260707_write_json_artifact_rejects_empty_json",
            &output,
        );

        assert!(result.is_err());
        assert!(!dir_exists);
    }

    #[test]
    fn test_20260707_write_json_artifact_rejects_pathy_crate_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-pathy-crate-{}-{unique}",
            std::process::id()
        ));

        let graph = rvs_test_callgraph();
        let slash = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("bad/name"), &graph);
        let empty = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from(""), &graph);
        let dir_exists = dir.exists();
        let output = format!(
            "slash_is_err={}\nempty_is_err={}\ndir_exists={dir_exists}\n",
            slash.is_err(),
            empty.is_err()
        );
        rvs_snapshot_BIS(
            "test_20260707_write_json_artifact_rejects_pathy_crate_name",
            &output,
        );

        assert!(slash.is_err());
        assert!(empty.is_err());
        assert!(!dir_exists);
    }

    #[test]
    fn test_20260707_write_json_artifact_rejects_nul_crate_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-nul-crate-{}-{unique}",
            std::process::id()
        ));

        let graph = rvs_test_callgraph();
        let result = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("bad\0name"), &graph);
        let dir_exists = dir.exists();
        let output = format!("is_err={}\ndir_exists={dir_exists}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260707_write_json_artifact_rejects_nul_crate_name",
            &output,
        );

        assert!(result.is_err());
        assert!(!dir_exists);
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_write_json_artifact_skips_preexisting_tmp_symlink() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-symlink-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, "victim\n").unwrap();
        let predictable_tmp = dir.join(format!("demo-{}.json.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, &predictable_tmp).unwrap();

        let graph = rvs_test_callgraph();
        let path = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("demo"), &graph)
            .expect("artifact write should succeed through a retry temp path");
        let victim_content = std::fs::read_to_string(&victim).unwrap();
        let symlink_still_exists = std::fs::symlink_metadata(&predictable_tmp).is_ok();
        let output = format!(
            "file_exists={}\nvictim_content={victim_content}\nsymlink_still_exists={symlink_still_exists}\n",
            path.is_file()
        );
        rvs_snapshot_BIS(
            "test_20260706_write_json_artifact_skips_preexisting_tmp_symlink",
            &output,
        );

        assert_eq!(victim_content, "victim\n");
        assert!(symlink_still_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_write_json_artifact_removes_tmp_on_rename_error() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rivus-artifact-rename-error-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join(format!("demo-{}.json", std::process::id()))).unwrap();

        let graph = rvs_test_callgraph();
        let result = rvs_write_callgraph_artifact_BIS(&dir, &CrateName::from("demo"), &graph);
        let tmp_exists = dir
            .join(format!("demo-{}.json.tmp", std::process::id()))
            .exists();
        let output = format!("is_err={}\ntmp_exists={tmp_exists}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260706_write_json_artifact_removes_tmp_on_rename_error",
            &output,
        );

        assert!(result.is_err());
        assert!(!tmp_exists);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
