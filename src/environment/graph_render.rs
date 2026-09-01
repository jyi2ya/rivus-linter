//! Parent-process rendering of merged-graph diagnostics.
//!
//! The offline engine produces structured emissions with fixed severities;
//! this adapter resolves each anchor onto artifact-recorded source
//! locations and renders plain-text diagnostics. It is the single
//! renderer of `cargo rivus check` graph diagnostics — rustc lint levels
//! never apply to them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::artifacts::FnGraph;
use crate::diagnostic_source::{
    DiagnosticLocation, rvs_resolve_call_site_anchor, rvs_resolve_node_anchor,
};
use crate::offline_caps::{OfflineCapsEmission, OfflineCapsSeverity};

/// Rendered graph diagnostics plus the fixed-severity counts that decide
/// the check exit code.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderedGraphDiagnostics {
    pub(crate) output: String,
    pub(crate) error_count: usize,
    pub(crate) warning_count: usize,
}

/// Renders every emission: a header line per diagnostic, then one located
/// block per anchor. Anchors that resolve to no source location render by
/// readable def path — a graph diagnostic is never silently dropped.
pub(crate) fn rvs_render_graph_emissions_BIS(
    graph: &FnGraph,
    emissions: &[OfflineCapsEmission],
) -> RenderedGraphDiagnostics {
    let mut rendered = RenderedGraphDiagnostics::default();
    let mut file_cache: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
    for emission in emissions {
        let severity = match emission.severity {
            OfflineCapsSeverity::Error => {
                rendered.error_count += 1;
                "error"
            }
            OfflineCapsSeverity::Warning => {
                rendered.warning_count += 1;
                "warning"
            }
        };
        writeln!(
            rendered.output,
            "{}[{}]: {}",
            severity,
            emission.lint.rvs_as_str(),
            emission.message
        )
        .expect("never: writing to String cannot fail");
        for anchor in &emission.span_anchors {
            let locations = match &anchor.call_site {
                Some(call_site) => rvs_resolve_call_site_anchor(call_site),
                None => rvs_resolve_node_anchor(graph, &anchor.identity),
            };
            if locations.is_empty() {
                writeln!(
                    rendered.output,
                    "  at {} (no source location recorded)",
                    anchor.identity.def_path.rvs_user_path()
                )
                .expect("never: writing to String cannot fail");
                continue;
            }
            for location in &locations {
                rvs_write_location_block_BIMS(&mut rendered.output, location, &mut file_cache);
            }
        }
    }
    rendered
}

fn rvs_write_location_block_BIMS(
    output: &mut String,
    location: &DiagnosticLocation,
    file_cache: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
) {
    let content = file_cache
        .entry(location.file.clone())
        .or_insert_with(|| std::fs::read(&location.file).ok())
        .as_deref();
    let (line_number, column) = content
        .and_then(|content| rvs_line_col(content, location.start))
        .map_or((None, None), |(line, column)| (Some(line), Some(column)));
    match (line_number, column) {
        (Some(line), Some(column)) => {
            writeln!(
                output,
                "  --> {}:{}:{} (bytes {}..{})",
                location.file.display(),
                line,
                column,
                location.start,
                location.end
            )
            .expect("never: writing to String cannot fail");
            if let Some(content) = content {
                rvs_write_snippet_block_M(output, content, location, line, column);
            }
        }
        _ => {
            writeln!(
                output,
                "  --> {} (bytes {}..{})",
                location.file.display(),
                location.start,
                location.end
            )
            .expect("never: writing to String cannot fail");
        }
    }
}

/// Writes the offending source line and a caret underline. Spans crossing
/// a line break underline up to the end of the first line only.
fn rvs_write_snippet_block_M(
    output: &mut String,
    content: &[u8],
    location: &DiagnosticLocation,
    line_number: usize,
    column: usize,
) {
    let Some(line_start) = rvs_line_start(content, location.start) else {
        return;
    };
    let line_end = content
        .get(line_start..)
        .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
        .map_or(content.len(), |offset| line_start + offset);
    let line_text =
        String::from_utf8_lossy(content.get(line_start..line_end).unwrap_or_default()).into_owned();
    let gutter_width = line_number.to_string().len();
    let pad = " ".repeat(gutter_width);
    writeln!(output, "{pad} |").expect("never: writing to String cannot fail");
    writeln!(output, "{line_number} | {line_text}").expect("never: writing to String cannot fail");
    let highlight_end =
        (usize::try_from(location.end).expect("never: offset fits usize")).min(line_end);
    let caret_count = highlight_end
        .saturating_sub(usize::try_from(location.start).expect("never: offset fits usize"))
        .max(1);
    let indent = " ".repeat(column.saturating_sub(1));
    let carets = "^".repeat(caret_count);
    writeln!(output, "{pad} | {indent}{carets}").expect("never: writing to String cannot fail");
}

/// One-based line and column of a byte offset, or `None` when the offset
/// lies outside the content.
fn rvs_line_col(content: &[u8], offset: u32) -> Option<(usize, usize)> {
    let offset = usize::try_from(offset).ok()?;
    if offset > content.len() {
        return None;
    }
    let mut line = 1usize;
    let mut line_start = 0usize;
    for (index, byte) in content.iter().enumerate().take(offset) {
        if *byte == b'\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    let column = content.get(line_start..offset)?.len() + 1;
    Some((line, column))
}

fn rvs_line_start(content: &[u8], offset: u32) -> Option<usize> {
    let offset = usize::try_from(offset).ok()?;
    if offset > content.len() {
        return None;
    }
    let mut line_start = 0usize;
    for (index, byte) in content.iter().enumerate().take(offset) {
        if *byte == b'\n' {
            line_start = index + 1;
        }
    }
    Some(line_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{FnNode, FnSource, FunctionIdentity};
    use crate::offline_caps::{OfflineCapsEmissionAnchor, OfflineCapsLint};
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};
    use std::collections::BTreeSet;

    fn rvs_source_graph_BIS(dir: &std::path::Path) -> FnGraph {
        std::fs::create_dir_all(dir.join("src")).expect("never: fixture dir creates");
        std::fs::write(
            dir.join("src/lib.rs"),
            "mod other;\n\npub fn rvs_flag() -> u32 {\n    7\n}\n",
        )
        .expect("never: fixture file writes");
        // "rvs_flag" ident range within src/lib.rs.
        let content = std::fs::read(dir.join("src/lib.rs")).expect("never: fixture reads back");
        let name_start = content
            .windows(8)
            .position(|window| window == b"rvs_flag")
            .expect("never: fixture contains the ident");
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.sources = BTreeSet::from([FnSource::rvs_new_relative(
            std::path::PathBuf::from("src/lib.rs"),
            dir.to_path_buf(),
            u32::try_from(name_start).expect("never: offset fits u32"),
            u32::try_from(name_start + 8).expect("never: offset fits u32"),
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_flag"), node);
        graph
    }

    #[test]
    fn test_20260901_renders_node_anchor_and_def_path_fallback() {
        let dir = rvs_make_temp_dir_BIS("graph-render");
        let graph = rvs_source_graph_BIS(&dir);
        let anchored = OfflineCapsEmission {
            lint: OfflineCapsLint::ContractMismatch,
            severity: OfflineCapsSeverity::Error,
            span_anchors: BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 7,
                    def_path: DefPath::from("demo::rvs_flag"),
                },
                call_site: None,
            }]),
            message: "fn 'rvs_flag' is missing capability marker missing_side_effect".to_string(),
        };
        let fallback = OfflineCapsEmission {
            lint: OfflineCapsLint::UntestedGoodFn,
            severity: OfflineCapsSeverity::Warning,
            span_anchors: BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 9,
                    def_path: DefPath::from("demo::rvs_sourceless"),
                },
                call_site: None,
            }]),
            message: "good fn 'rvs_sourceless' not called by any test".to_string(),
        };
        let rendered = rvs_render_graph_emissions_BIS(&graph, &[anchored, fallback]);
        let output = rendered
            .output
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260901_renders_node_anchor_and_def_path_fallback",
            &output,
        );

        assert!(rendered.output.contains("error[contract_mismatch]:"));
        assert!(rendered.output.contains("warning[untested_good_fn]:"));
        assert!(rendered.output.contains("rvs_flag"));
        assert!(
            rendered
                .output
                .contains("at demo::rvs_sourceless (no source location recorded)")
        );
        assert_eq!(rendered.error_count, 1);
        assert_eq!(rendered.warning_count, 1);
        std::fs::remove_dir_all(dir).expect("never: temp dir removes");
    }

    #[test]
    fn test_20260901_line_col_maps_offsets_to_one_based_positions() {
        let content = b"ab\ncdef\ngh";
        let output = format!(
            "first={:?}\nsecond={:?}\npast_end={:?}\n",
            rvs_line_col(content, 0),
            rvs_line_col(content, 4),
            rvs_line_col(content, 99),
        );
        rvs_snapshot_BIS(
            "test_20260901_line_col_maps_offsets_to_one_based_positions",
            &output,
        );

        assert_eq!(rvs_line_col(content, 0), Some((1, 1)));
        assert_eq!(rvs_line_col(content, 4), Some((2, 2)));
        assert_eq!(rvs_line_col(content, 99), None);
    }
}
