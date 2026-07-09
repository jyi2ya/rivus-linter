use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::artifacts::{self, FnGraph};
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet};
use crate::inference::{
    FnContractDiff, FnContractMismatch, FnContractMismatchKind,
    rvs_collect_contract_mismatch_items, rvs_collect_enforced_contract_diffs,
    rvs_collect_local_contract_diffs_M, rvs_summarize_contract_mismatches,
};
use crate::symbols::CrateName;
use crate::workspace::{
    CargoCheckConfig, CargoCheckError, rvs_clean_dir_BIS, rvs_collect_callgraph_and_caps_BIMS,
    rvs_ensure_cargo_project_BIS, rvs_load_local_crate_prefixes_BIS, rvs_run_cargo_check_impl_BIMS,
};

#[derive(Debug, Clone, Default)]
struct CapStats {
    fn_count: usize,
    line_count: usize,
}

#[derive(Debug, Clone)]
struct Report {
    by_capability: BTreeMap<Capability, CapStats>,
    pure_fn_count: usize,
    pure_line_count: usize,
    good_fn_count: usize,
    good_line_count: usize,
    ok_fn_count: usize,
    ok_line_count: usize,
    total_fn_count: usize,
    total_line_count: usize,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Capability Report")?;
        writeln!(f, "{:-<60}", "")?;
        writeln!(
            f,
            "Total: {} functions, {} lines",
            self.total_fn_count, self.total_line_count
        )?;
        writeln!(f, "{:-<60}", "")?;

        if self.total_fn_count == 0 {
            writeln!(f, "(no rvs_ functions found)")?;
            return Ok(());
        }

        let bar_width = 30;
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        rows.push(("(ok)".to_string(), self.ok_fn_count, self.ok_line_count));
        rows.push((
            "(good)".to_string(),
            self.good_fn_count,
            self.good_line_count,
        ));
        rows.push((
            "(pure)".to_string(),
            self.pure_fn_count,
            self.pure_line_count,
        ));

        for cap in [
            Capability::A,
            Capability::B,
            Capability::I,
            Capability::M,
            Capability::P,
            Capability::S,
            Capability::T,
            Capability::U,
        ] {
            if let Some(stats) = self.by_capability.get(&cap) {
                rows.push((cap.to_string(), stats.fn_count, stats.line_count));
            }
        }
        rows.sort_by_key(|b| std::cmp::Reverse(b.2));

        for (label, fn_count, line_count) in &rows {
            let pct = if self.total_line_count == 0 {
                0.0
            } else {
                *line_count as f64 / self.total_line_count as f64 * 100.0
            };
            #[expect(clippy::cast_sign_loss, reason = "pct is 0..=100")]
            let bar_len = (pct / 100.0 * bar_width as f64)
                .round()
                .clamp(0.0, bar_width as f64) as usize;
            let bar: String = "\u{2588}".repeat(bar_len) + &"\u{2591}".repeat(bar_width - bar_len);
            writeln!(
                f,
                "  {:<12} {:>5} fns {:>6} lines {:>6}% |{}|",
                label,
                fn_count,
                line_count,
                format!("{pct:.1}"),
                bar
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FnEntry {
    capabilities: CapabilitySet,
    line_count: usize,
    is_test: bool,
    allows_dead_code: bool,
}

fn rvs_build_report(entries: &[FnEntry]) -> Result<Report, String> {
    let mut by_capability: BTreeMap<Capability, CapStats> = BTreeMap::new();
    let mut pure_fn_count = 0usize;
    let mut pure_line_count = 0usize;
    let mut good_fn_count = 0usize;
    let mut good_line_count = 0usize;
    let mut ok_fn_count = 0usize;
    let mut ok_line_count = 0usize;
    let mut total_fn_count = 0usize;
    let mut total_line_count = 0usize;
    for func in entries {
        if func.is_test || func.allows_dead_code {
            continue;
        }
        if func.line_count == 0 {
            return Err("report entry line count must be positive".into());
        }
        total_fn_count = rvs_checked_report_sum(total_fn_count, 1, "total function count")?;
        total_line_count =
            rvs_checked_report_sum(total_line_count, func.line_count, "total report line count")?;

        if func.capabilities.rvs_is_empty() {
            pure_fn_count = rvs_checked_report_sum(pure_fn_count, 1, "pure function count")?;
            pure_line_count =
                rvs_checked_report_sum(pure_line_count, func.line_count, "pure report line count")?;
        } else {
            for cap in func.capabilities.rvs_iter() {
                let stats = by_capability.entry(cap).or_default();
                stats.fn_count =
                    rvs_checked_report_sum(stats.fn_count, 1, "capability function count")?;
                stats.line_count = rvs_checked_report_sum(
                    stats.line_count,
                    func.line_count,
                    "capability report line count",
                )?;
            }
        }

        if CapabilityPolicy::rvs_is_good(&func.capabilities) {
            good_fn_count = rvs_checked_report_sum(good_fn_count, 1, "good function count")?;
            good_line_count =
                rvs_checked_report_sum(good_line_count, func.line_count, "good report line count")?;
        }

        if CapabilityPolicy::rvs_is_ok(&func.capabilities) {
            ok_fn_count = rvs_checked_report_sum(ok_fn_count, 1, "ok function count")?;
            ok_line_count =
                rvs_checked_report_sum(ok_line_count, func.line_count, "ok report line count")?;
        }
    }

    Ok(Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        ok_fn_count,
        ok_line_count,
        total_fn_count,
        total_line_count,
    })
}

fn rvs_checked_report_sum(current: usize, delta: usize, label: &str) -> Result<usize, String> {
    debug_assert!(current.checked_add(0).is_some(), "current count is valid");
    debug_assert!(delta.checked_add(0).is_some(), "delta count is valid");
    current
        .checked_add(delta)
        .ok_or_else(|| format!("{label} overflow while building capability report"))
}

fn rvs_read_report_entries_with_count_BIS(
    report_dir: &Path,
) -> Result<(Vec<FnEntry>, usize), String> {
    let rd = match std::fs::read_dir(report_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(report_dir) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok((Vec::new(), 0));
                }
                Ok(_) => {
                    return Err(format!(
                        "report dir must be a directory: {}",
                        report_dir.display()
                    ));
                }
                Err(symlink_error) => {
                    return Err(format!(
                        "cannot inspect report dir {}: {symlink_error}",
                        report_dir.display()
                    ));
                }
            }
        }
        Err(e) => return Err(format!("cannot read {}: {e}", report_dir.display())),
    };

    let mut json_paths = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| format!("readdir error in {}: {e}", report_dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", entry.path().display()))?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        json_paths.push(path);
    }
    json_paths.sort();

    let artifact_count = json_paths.len();
    let mut all_entries = Vec::new();
    for path in json_paths {
        let json_str = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let entries = artifacts::rvs_parse_report_json_S(&json_str)
            .map_err(|e| format!("parsing {}: {e}", path.display()))?;
        for entry in entries {
            let capabilities = if entry.caps.is_empty() {
                CapabilitySet::rvs_new()
            } else {
                CapabilitySet::rvs_from_str(&entry.caps).map_err(|e| {
                    format!(
                        "{}: invalid capability string '{}' for {}: {e}",
                        path.display(),
                        entry.caps,
                        entry.name
                    )
                })?
            };
            all_entries.push(FnEntry {
                capabilities,
                line_count: entry.lines,
                is_test: entry.is_test,
                allows_dead_code: entry.allows_dead_code,
            });
        }
    }
    Ok((all_entries, artifact_count))
}

#[cfg(test)]
fn rvs_read_report_entries_BIS(report_dir: &Path) -> Result<Vec<FnEntry>, String> {
    rvs_read_report_entries_with_count_BIS(report_dir).map(|(entries, _)| entries)
}

fn rvs_read_required_report_entries_BIS(report_dir: &Path) -> Result<Vec<FnEntry>, String> {
    let (entries, artifact_count) = rvs_read_report_entries_with_count_BIS(report_dir)?;
    if artifact_count == 0 {
        return Err(format!(
            "no report JSON artifacts found in {}",
            report_dir.display()
        ));
    }
    Ok(entries)
}

fn rvs_read_report_entries_after_cargo_BIS(
    report_dir: &Path,
    cargo_check: Result<(), CargoCheckError>,
) -> Result<Vec<FnEntry>, String> {
    match cargo_check {
        Ok(()) => rvs_read_required_report_entries_BIS(report_dir),
        Err(e) => {
            // Report mode should still produce output even if lint violations
            // (deny-level errors) cause cargo check to fail. The report JSON
            // is written by the lint pass before compilation aborts.
            let (entries, artifact_count) = rvs_read_report_entries_with_count_BIS(report_dir)?;
            if artifact_count == 0 {
                return Err(e.to_string());
            }
            eprintln!("warning: {e}");
            Ok(entries)
        }
    }
}

fn rvs_format_contract_mismatch_summary(
    counts: &BTreeMap<FnContractMismatchKind, usize>,
    items: &[FnContractMismatch],
) -> String {
    if counts.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nContract Mismatches\n------------------------------\n");
    for (kind, count) in counts {
        out.push_str(&format!("{:<24} {}\n", kind.rvs_as_str(), count));
    }
    out.push_str("\nSample Mismatches\n------------------------------\n");
    for item in items.iter().take(10) {
        out.push_str(&format!("{}: {}\n", item.kind.rvs_as_str(), item.def_path));
    }
    out
}

fn rvs_collect_reportable_contract_diffs(
    graph: &FnGraph,
    diffs: &[FnContractDiff],
    local_crate_names: &std::collections::BTreeSet<CrateName>,
) -> Vec<FnContractDiff> {
    rvs_collect_enforced_contract_diffs(graph, diffs, local_crate_names)
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_report_BIMPS(path: &Path) -> Result<(), String> {
    rvs_ensure_cargo_project_BIS(path)?;

    let report_dir = path.join("target").join("rivus-report");
    let abs_report_dir = std::env::current_dir()
        .map_err(|e| format!("current dir invalid: {e}"))?
        .join(&report_dir);
    rvs_clean_dir_BIS(&report_dir)?;
    rvs_clean_dir_BIS(&path.join("target").join("rivus-report-build"))?;

    let cargo_check = rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates: false,
        with_tests: true,
        build_std: false,
        user_capsmap: None,
        extra_env: vec![
            ("RIVUS_REPORT", "1".into()),
            ("RIVUS_REPORT_DIR", abs_report_dir.into_os_string()),
        ],
        extra_args: vec![],
        target_subdir: Some("rivus-report-build"),
    });
    let report_entries = rvs_read_report_entries_after_cargo_BIS(&report_dir, cargo_check)?;

    let report = rvs_build_report(&report_entries)?;
    let mismatch_output = match rvs_collect_report_contract_mismatches_BIMPS(path) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("warning: contract mismatch report unavailable: {e}");
            String::new()
        }
    };
    print!("{report}");
    if !mismatch_output.is_empty() {
        print!("{mismatch_output}");
    }
    Ok(())
}

fn rvs_collect_report_contract_mismatches_BIMPS(path: &Path) -> Result<String, String> {
    let local_crate_names = rvs_load_local_crate_prefixes_BIS(path)?;
    let (mut callgraph, caps) = rvs_collect_callgraph_and_caps_BIMS(path, true)?;
    let diffs = rvs_collect_local_contract_diffs_M(&mut callgraph, &caps, &local_crate_names);
    let reportable_diffs =
        rvs_collect_reportable_contract_diffs(&callgraph, &diffs, &local_crate_names);
    let mismatch_items = rvs_collect_contract_mismatch_items(&reportable_diffs);
    let mismatch_summary = rvs_summarize_contract_mismatches(&reportable_diffs);
    Ok(rvs_format_contract_mismatch_summary(
        &mismatch_summary,
        &mismatch_items,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::FnNode;
    use crate::symbols::DefPath;

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    fn rvs_make_temp_dir_BIS(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("never: system clock should be after unix epoch for test temp dir")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rivus-{tag}-{}-{unique}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_20260607_report_empty() {
        let entries = vec![];
        let report = rvs_build_report(&entries).unwrap();
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_empty", &output);
        assert_eq!(report.total_fn_count, 0);
        assert_eq!(report.total_line_count, 0);
    }

    #[test]
    fn test_20260706_read_report_entries_missing_dir_is_empty() {
        let dir = rvs_make_temp_dir_BIS("missing-report-dir");
        let missing = dir.join("target/rivus-report");

        let result = rvs_read_report_entries_BIS(&missing);
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_missing_dir_is_empty",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(matches!(result, Ok(entries) if entries.is_empty()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_read_report_entries_rejects_broken_symlink_dir() {
        let dir = rvs_make_temp_dir_BIS("report-broken-symlink-dir");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let report_path = dir.join("target/rivus-report");
        std::os::unix::fs::symlink(dir.join("missing-report-dir"), &report_path).unwrap();

        let result = rvs_read_report_entries_BIS(&report_path);
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_rejects_broken_symlink_dir",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_read_report_entries_rejects_report_file_path() {
        let dir = rvs_make_temp_dir_BIS("report-dir-is-file");
        let report_path = dir.join("target/rivus-report");
        std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        std::fs::write(&report_path, "not a directory\n").unwrap();

        let result = rvs_read_report_entries_BIS(&report_path);
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_rejects_report_file_path",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_read_report_entries_rejects_invalid_caps() {
        let dir = rvs_make_temp_dir_BIS("report-invalid-caps");
        let report_dir = dir.join("target/rivus-report");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("demo-1.json"),
            r#"[{"name":"rvs_bad_Z","caps":"Z","lines":1,"is_test":false,"allows_dead_code":false}]"#,
        )
        .unwrap();

        let result = rvs_read_report_entries_BIS(&report_dir);
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_rejects_invalid_caps",
            &format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260607_report_pure_only() {
        let entries = vec![FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            line_count: 10,
            is_test: false,
            allows_dead_code: false,
        }];
        let report = rvs_build_report(&entries).unwrap();
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_pure_only", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 1);
        assert_eq!(report.ok_fn_count, 1);
    }

    #[test]
    fn test_20260607_report_mixed() {
        let entries = vec![
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 100,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_from_validated("BI"),
                line_count: 50,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_from_validated("M"),
                line_count: 30,
                is_test: false,
                allows_dead_code: false,
            },
        ];
        let report = rvs_build_report(&entries).unwrap();
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_mixed", &output);
        assert_eq!(report.total_fn_count, 3);
        assert_eq!(report.pure_fn_count, 1);
        assert_eq!(report.good_fn_count, 2);
        assert_eq!(report.ok_fn_count, 2);
        assert_eq!(report.total_line_count, 180);
    }

    #[test]
    fn test_20260607_report_skips_test_and_dead_code() {
        let entries = vec![
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 10,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 20,
                is_test: true,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 30,
                is_test: false,
                allows_dead_code: true,
            },
        ];
        let report = rvs_build_report(&entries).unwrap();
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_skips_test_and_dead_code", &output);
        assert_eq!(report.total_fn_count, 1);
        assert_eq!(report.total_line_count, 10);
    }

    #[test]
    fn test_20260608_json_parse_empty() {
        let entries = artifacts::rvs_parse_report_json_S("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_20260608_json_parse_single_pure() {
        let json =
            r#"[{"name":"rvs_add","caps":"","lines":5,"is_test":false,"allows_dead_code":false}]"#;
        let entries = artifacts::rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].caps.is_empty());
        assert_eq!(entries[0].lines, 5);
        assert!(!entries[0].is_test);
    }

    #[test]
    fn test_20260608_json_parse_with_caps() {
        let json = r#"[{"name":"rvs_write_BI","caps":"BI","lines":10,"is_test":false,"allows_dead_code":false}]"#;
        let entries = artifacts::rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].caps, "BI");
    }

    #[test]
    fn test_20260608_json_parse_test_fn() {
        let json = r#"[{"name":"test_20260608_foo","caps":"S","lines":3,"is_test":true,"allows_dead_code":false}]"#;
        let entries = artifacts::rvs_parse_report_json_S(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_test);
    }

    #[test]
    fn test_20260702_report_rejects_non_cargo_dir() {
        let dir = rvs_make_temp_dir_BIS("report-non-cargo");
        let result = rvs_run_report_BIMPS(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260702_report_rejects_non_cargo_dir", &output);
        assert!(result.is_err(), "report should fail for non-cargo dir");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260703_format_contract_mismatch_summary() {
        let counts = BTreeMap::from([
            (FnContractMismatchKind::MissingPort, 2usize),
            (FnContractMismatchKind::NameMismatch, 1usize),
        ]);
        let items = vec![
            FnContractMismatch {
                def_path: crate::symbols::DefPath::from("demo::rvs_fetch_BI"),
                actual_name: crate::symbols::FnName::from("rvs_fetch_BI"),
                kind: FnContractMismatchKind::MissingPort,
            },
            FnContractMismatch {
                def_path: crate::symbols::DefPath::from("demo::rvs_fetch_BI"),
                actual_name: crate::symbols::FnName::from("rvs_fetch_BI"),
                kind: FnContractMismatchKind::NameMismatch,
            },
        ];
        let output = rvs_format_contract_mismatch_summary(&counts, &items);
        rvs_snapshot_BIS("test_20260703_format_contract_mismatch_summary", &output);

        assert!(output.contains("Contract Mismatches"));
        assert!(output.contains("Sample Mismatches"));
        assert!(output.contains("missing_port"));
        assert!(output.contains("name_mismatch"));
    }

    #[test]
    fn test_20260703_collect_reportable_contract_diffs_keeps_nested_main() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(DefPath::from("demo::main"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::cli::main"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::User::new"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::go"), FnNode::default());
        graph.rvs_insert_M(DefPath::from("demo::wblk"), FnNode::default());
        graph.rvs_insert_M(
            DefPath::from("demo::Widget::sync@demo::Syncer"),
            FnNode {
                is_trait_impl: true,
                ..FnNode::default()
            },
        );
        graph.rvs_insert_M(DefPath::from("demo::parse"), FnNode::default());

        let diffs = vec![
            FnContractDiff {
                def_path: DefPath::from("demo::main"),
                actual_name: crate::symbols::FnName::from("main"),
                expected_name: Some(crate::symbols::FnName::from("rvs_main_BI")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::cli::main"),
                actual_name: crate::symbols::FnName::from("main"),
                expected_name: Some(crate::symbols::FnName::from("rvs_main")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::User::new"),
                actual_name: crate::symbols::FnName::from("new"),
                expected_name: Some(crate::symbols::FnName::from("rvs_new")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::go"),
                actual_name: crate::symbols::FnName::from("go"),
                expected_name: Some(crate::symbols::FnName::from("rvs_go")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::wblk"),
                actual_name: crate::symbols::FnName::from("wblk"),
                expected_name: Some(crate::symbols::FnName::from("rvs_wblk")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::Widget::sync@demo::Syncer"),
                actual_name: crate::symbols::FnName::from("rvs_sync_BI"),
                expected_name: Some(crate::symbols::FnName::from("rvs_sync_P")),
                declared_public_caps: Some(CapabilitySet::rvs_from_validated("BI")),
                expected_public_caps: Some(CapabilitySet::rvs_from_validated("P")),
            },
            FnContractDiff {
                def_path: DefPath::from("demo::parse"),
                actual_name: crate::symbols::FnName::from("parse"),
                expected_name: Some(crate::symbols::FnName::from("rvs_parse")),
                declared_public_caps: None,
                expected_public_caps: Some(CapabilitySet::rvs_new()),
            },
        ];

        let filtered = rvs_collect_reportable_contract_diffs(
            &graph,
            &diffs,
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        );
        let output = format!(
            "filtered={filtered:?}\nsummary={:?}\n",
            rvs_summarize_contract_mismatches(&filtered)
        );
        rvs_snapshot_BIS(
            "test_20260703_collect_reportable_contract_diffs_keeps_nested_main",
            &output,
        );

        assert_eq!(filtered.len(), 5);
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::cli::main")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::User::new")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::go")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::wblk")
        );
        assert!(
            filtered
                .iter()
                .any(|diff| diff.def_path.rvs_as_str() == "demo::parse")
        );
    }

    #[test]
    fn test_20260707_build_report_rejects_zero_line_count() {
        let result = rvs_build_report(&[FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            line_count: 0,
            is_test: false,
            allows_dead_code: false,
        }]);
        rvs_snapshot_BIS(
            "test_20260707_build_report_rejects_zero_line_count",
            &format!("{result:?}\n"),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_20260706_build_report_rejects_line_count_overflow() {
        let entries = vec![
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: usize::MAX,
                is_test: false,
                allows_dead_code: false,
            },
            FnEntry {
                capabilities: CapabilitySet::rvs_new(),
                line_count: 1,
                is_test: false,
                allows_dead_code: false,
            },
        ];

        let result = rvs_build_report(&entries);
        rvs_snapshot_BIS(
            "test_20260706_build_report_rejects_line_count_overflow",
            &format!("{result:?}\n"),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_20260706_checked_report_sum_handles_ok_and_overflow() {
        let ok = rvs_checked_report_sum(2, 3, "demo");
        let overflow = rvs_checked_report_sum(usize::MAX, 1, "demo");
        rvs_snapshot_BIS(
            "test_20260706_checked_report_sum_handles_ok_and_overflow",
            &format!("ok={ok:?}\noverflow={overflow:?}\n"),
        );

        assert_eq!(ok, Ok(5));
        assert!(overflow.is_err());
    }

    #[test]
    fn test_20260706_read_report_entries_sorts_json_files() {
        let dir = rvs_make_temp_dir_BIS("report-sorted-json");
        std::fs::write(
            dir.join("z.json"),
            r#"[{"name":"rvs_z","caps":"","lines":2,"is_test":false,"allows_dead_code":false}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("a.json"),
            r#"[{"name":"rvs_a","caps":"","lines":1,"is_test":false,"allows_dead_code":false}]"#,
        )
        .unwrap();

        let entries = rvs_read_report_entries_BIS(&dir).unwrap();
        let lines = entries
            .iter()
            .map(|entry| entry.line_count.to_string())
            .collect::<Vec<_>>()
            .join(",");
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_sorts_json_files",
            &format!("lines={lines}\n"),
        );

        assert_eq!(lines, "1,2");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_read_report_entries_ignores_json_directory() {
        let dir = rvs_make_temp_dir_BIS("report-json-directory");
        std::fs::create_dir_all(dir.join("a.json")).unwrap();
        std::fs::write(
            dir.join("b.json"),
            r#"[{"name":"rvs_b","caps":"","lines":3,"is_test":false,"allows_dead_code":false}]"#,
        )
        .unwrap();

        let entries = rvs_read_report_entries_BIS(&dir).unwrap();
        let output = format!("len={}\nlines={}\n", entries.len(), entries[0].line_count);
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_ignores_json_directory",
            &output,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].line_count, 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_read_report_entries_rejects_empty_function_name() {
        let dir = rvs_make_temp_dir_BIS("report-empty-name");
        std::fs::write(
            dir.join("entry.json"),
            r#"[{"name":"","caps":"","lines":1,"is_test":false,"allows_dead_code":false}]"#,
        )
        .unwrap();

        let result = rvs_read_report_entries_BIS(&dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_read_report_entries_rejects_empty_function_name",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("function name is empty"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_failed_cargo_with_empty_report_dir_returns_error() {
        let dir = rvs_make_temp_dir_BIS("report-empty-after-failed-cargo");
        std::fs::create_dir_all(&dir).unwrap();

        let result =
            rvs_read_report_entries_after_cargo_BIS(&dir, Err(CargoCheckError::ExitCode(101)));
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_failed_cargo_with_empty_report_dir_returns_error",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("cargo check failed"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260708_successful_cargo_requires_report_artifact() {
        let dir = rvs_make_temp_dir_BIS("report-missing-after-successful-cargo");
        let missing = dir.join("target/rivus-report");

        let result = rvs_read_report_entries_after_cargo_BIS(&missing, Ok(()));
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260708_successful_cargo_requires_report_artifact",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("no report JSON artifacts"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260708_failed_cargo_reports_wrong_type_report_path() {
        let dir = rvs_make_temp_dir_BIS("report-file-after-failed-cargo");
        let report_path = dir.join("target/rivus-report");
        std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        std::fs::write(&report_path, "not a directory\n").unwrap();

        let result = rvs_read_report_entries_after_cargo_BIS(
            &report_path,
            Err(CargoCheckError::ExitCode(101)),
        );
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260708_failed_cargo_reports_wrong_type_report_path",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("cannot read"));
        assert!(!output.contains("cargo check failed"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260708_run_report_accepts_async_fn_project() {
        let dir = rvs_make_temp_dir_BIS("report-async-project");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"report-async-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            concat!(
                "#![allow(non_snake_case)]\n",
                "pub async fn rvs_send_messages_AS(x: i32) -> i32 {\n",
                "    debug_assert!(x > 0);\n",
                "    if x > 1 { x } else { x + 1 }\n",
                "}\n",
                "\n",
                "#[cfg(test)]\n",
                "mod tests {\n",
                "    use super::*;\n",
                "\n",
                "    #[test]\n",
                "    fn test_20260708_async_report_project() {\n",
                "        std::mem::drop(rvs_send_messages_AS(1));\n",
                "    }\n",
                "}\n",
            ),
        )
        .unwrap();

        let result = rvs_run_report_BIMPS(&dir);
        let report_entries = rvs_read_required_report_entries_BIS(&dir.join("target/rivus-report"));
        let async_lines = report_entries.as_ref().ok().and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.capabilities == CapabilitySet::rvs_from_str("AS").unwrap())
                .map(|entry| entry.line_count)
        });
        let output = format!(
            "result={result:?}\nreport_entries={report_entries:?}\nasync_lines={async_lines:?}\n"
        )
        .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260708_run_report_accepts_async_fn_project", &output);

        assert!(result.is_ok(), "{output}");
        let entries = report_entries.expect("never: successful report run should produce entries");
        assert!(entries.iter().all(|entry| entry.line_count > 0), "{output}");
        assert_eq!(async_lines, Some(2), "{output}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
