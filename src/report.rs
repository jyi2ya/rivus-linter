use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::capability::{Capability, CapabilitySet};
use crate::extract::{rvs_extract_functions_E, FnDef};
use crate::source::{rvs_read_rust_sources_BEI, SourceFile};

/// 一种能力所占的份量：函数数、行数。
#[derive(Debug, Clone, Default)]
pub struct CapStats {
    pub fn_count: usize,
    pub line_count: usize,
}

/// 一份报告：按能力分组，各占多少行，几成天下的几分。
#[derive(Debug, Clone)]
pub struct Report {
    pub by_capability: BTreeMap<Capability, CapStats>,
    pub pure_fn_count: usize,
    pub pure_line_count: usize,
    pub good_fn_count: usize,
    pub good_line_count: usize,
    pub total_fn_count: usize,
    pub total_line_count: usize,
}

/// 纯函数：从一组函数定义中，统计各能力的份量。
///
/// 称斤掂两，一分一毫都不差。
/// 纯函数排一行，好函数排一行，
/// 八德各归其位，各算各的账。
pub fn rvs_build_report(functions: &[FnDef]) -> Report {
    let mut by_capability: BTreeMap<Capability, CapStats> = BTreeMap::new();
    let mut pure_fn_count = 0usize;
    let mut pure_line_count = 0usize;
    let mut good_fn_count = 0usize;
    let mut good_line_count = 0usize;
    let mut total_fn_count = 0usize;
    let mut total_line_count = 0usize;

    let good_allowed = CapabilitySet::rvs_from_good_caps();

    for func in functions {
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

        if func.capabilities.rvs_is_subset_of(&good_allowed) {
            good_fn_count += 1;
            good_line_count += func.line_count;
        }
    }

    Report {
        by_capability,
        pure_fn_count,
        pure_line_count,
        good_fn_count,
        good_line_count,
        total_fn_count,
        total_line_count,
    }
}

/// 从多个已读入的源文件中生成报告。
/// 可能失败：解析出错便报错。无 IO，干干净净。
#[allow(non_snake_case)]
pub fn rvs_report_sources_E(sources: &[SourceFile]) -> Result<Report, ReportError> {
    let mut all_functions = Vec::new();
    for sf in sources {
        let functions = rvs_extract_functions_E(&sf.source)
            .map_err(|e| ReportError::Extract {
                file: sf.path.clone(),
                source: e,
            })?;
        all_functions.extend(functions);
    }
    Ok(rvs_build_report(&all_functions))
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
            Capability::E,
            Capability::I,
            Capability::M,
            Capability::P,
            Capability::T,
            Capability::U,
        ] {
            if let Some(stats) = self.by_capability.get(&cap) {
                rows.push((cap.to_string(), stats.fn_count, stats.line_count));
            }
        }

        rows.sort_by(|a, b| b.2.cmp(&a.2));

        for (label, fn_count, line_count) in &rows {
            let pct = *line_count as f64 / self.total_line_count as f64 * 100.0;
            let bar_len = (pct / 100.0 * bar_width as f64).round() as usize;
            let bar: String = "█".repeat(bar_len)
                + &"░".repeat(bar_width - bar_len);
            writeln!(
                f,
                "  {:<12} {:>5} fns {:>6} lines {:>6}% |{}|",
                label,
                fn_count,
                line_count,
                format!("{pct:.1}"),
                bar,
            )?;
        }

        Ok(())
    }
}

/// 从文件路径（或目录）出发，生成汇报。
/// 薄薄一层壳：只管读文件，真正的事交给纯函数。
#[allow(non_snake_case)]
pub fn rvs_report_path_BEI(path: &Path) -> Result<Report, ReportError> {
    let sources = rvs_read_rust_sources_BEI(path)
        .map_err(|e| ReportError::Read { source: e })?;
    rvs_report_sources_E(&sources)
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("failed to read: {source}")]
    Read {
        source: crate::source::ReadError,
    },
    #[error("failed to extract from '{file}': {source}")]
    Extract {
        file: String,
        source: crate::extract::ExtractError,
    },
}
