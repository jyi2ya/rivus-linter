use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::artifacts;
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet};
use crate::workspace::{
    CargoCheckConfig, rvs_clean_dir_BIS, rvs_ensure_cargo_project_BIS,
    rvs_run_cargo_check_impl_BIMS,
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

        if self.total_line_count == 0 {
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
            let pct = *line_count as f64 / self.total_line_count as f64 * 100.0;
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

fn rvs_build_report(entries: &[FnEntry]) -> Report {
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
        total_fn_count += 1;
        total_line_count += func.line_count;

        if func.capabilities.rvs_is_empty() {
            pure_fn_count += 1;
            pure_line_count += func.line_count;
        } else {
            for cap in func.capabilities.rvs_iter() {
                let stats = by_capability.entry(cap).or_default();
                stats.fn_count += 1;
                stats.line_count += func.line_count;
            }
        }

        if CapabilityPolicy::rvs_is_good(&func.capabilities) {
            good_fn_count += 1;
            good_line_count += func.line_count;
        }

        if CapabilityPolicy::rvs_is_ok(&func.capabilities) {
            ok_fn_count += 1;
            ok_line_count += func.line_count;
        }
    }

    Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        ok_fn_count,
        ok_line_count,
        total_fn_count,
        total_line_count,
    }
}

fn rvs_read_report_entries_BIS(report_dir: &Path) -> Result<Vec<FnEntry>, String> {
    let Ok(rd) = std::fs::read_dir(report_dir) else {
        return Ok(Vec::new());
    };

    let mut all_entries = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let json_str = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let entries = artifacts::rvs_parse_report_json_S(&json_str)
            .map_err(|e| format!("parsing {}: {e}", path.display()))?;
        all_entries.extend(entries.into_iter().map(|entry| FnEntry {
            capabilities: if entry.caps.is_empty() {
                CapabilitySet::rvs_new()
            } else {
                CapabilitySet::rvs_from_validated(&entry.caps)
            },
            line_count: entry.lines,
            is_test: entry.is_test,
            allows_dead_code: entry.allows_dead_code,
        }));
    }
    Ok(all_entries)
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
    rvs_clean_dir_BIS(&report_dir);
    rvs_clean_dir_BIS(&path.join("target").join("rivus-report-build"));

    let cargo_check = rvs_run_cargo_check_impl_BIMS(&CargoCheckConfig {
        project_path: path,
        wrap_all_crates: false,
        with_tests: true,
        build_std: false,
        user_capsmap: None,
        extra_env: vec![
            ("RIVUS_REPORT", "1".into()),
            (
                "RIVUS_REPORT_DIR",
                abs_report_dir.to_string_lossy().into_owned(),
            ),
        ],
        extra_args: vec![],
        target_subdir: Some("rivus-report-build"),
    });
    if let Err(e) = cargo_check {
        // Report mode should still produce output even if lint violations
        // (deny-level errors) cause cargo check to fail. The report JSON
        // is written by the lint pass before compilation aborts.
        if !report_dir.is_dir() {
            return Err(e);
        }
        eprintln!("warning: {e}");
    }

    let report = rvs_build_report(&rvs_read_report_entries_BIS(&report_dir)?);
    print!("{report}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let report = rvs_build_report(&entries);
        let output = report.to_string();
        rvs_snapshot_BIS("test_20260607_report_empty", &output);
        assert_eq!(report.total_fn_count, 0);
        assert_eq!(report.total_line_count, 0);
    }

    #[test]
    fn test_20260607_report_pure_only() {
        let entries = vec![FnEntry {
            capabilities: CapabilitySet::rvs_new(),
            line_count: 10,
            is_test: false,
            allows_dead_code: false,
        }];
        let report = rvs_build_report(&entries);
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
        let report = rvs_build_report(&entries);
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
        let report = rvs_build_report(&entries);
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
        rvs_snapshot_BIS(
            "test_20260702_report_rejects_non_cargo_dir",
            &format!("{result:?}"),
        );
        assert!(result.is_err(), "report should fail for non-cargo dir");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
