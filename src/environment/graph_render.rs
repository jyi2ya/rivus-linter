//! Parent-process rendering of merged-graph diagnostics.
//!
//! The offline engine produces structured emissions with fixed severities;
//! this adapter resolves each anchor onto artifact-recorded source
//! locations and renders plain-text diagnostics with the
//! `annotate-snippets` renderer (the library rustc itself uses). It is the
//! single renderer of `cargo rivus check` graph diagnostics — rustc lint
//! levels never apply to them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use annotate_snippets::{Level, Renderer, Snippet};

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

/// One resolved anchor whose source could be read, held as owned data so
/// the borrowed `Snippet` view can be built right before rendering.
struct LocatedSnippet {
    source: String,
    origin: String,
    byte_label: String,
    start: usize,
    end: usize,
}

/// Renders every emission: a header line per diagnostic
/// (`{severity}[{lint}]: {message}`), then one annotated snippet per
/// resolvable anchor. Anchors that resolve to no source location render by
/// readable def path — a graph diagnostic is never silently dropped.
pub(crate) fn rvs_render_graph_emissions_BIS(
    graph: &FnGraph,
    emissions: &[OfflineCapsEmission],
) -> RenderedGraphDiagnostics {
    let mut rendered = RenderedGraphDiagnostics::default();
    let mut file_cache: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    let renderer = Renderer::plain();
    for emission in emissions {
        let level = match emission.severity {
            OfflineCapsSeverity::Error => {
                rendered.error_count += 1;
                Level::Error
            }
            OfflineCapsSeverity::Warning => {
                rendered.warning_count += 1;
                Level::Warning
            }
        };
        let mut located: Vec<LocatedSnippet> = Vec::new();
        let mut fallback_lines: Vec<String> = Vec::new();
        for anchor in &emission.span_anchors {
            let locations = match &anchor.call_site {
                Some(call_site) => rvs_resolve_call_site_anchor(call_site),
                None => rvs_resolve_node_anchor(graph, &anchor.identity),
            };
            if locations.is_empty() {
                fallback_lines.push(format!(
                    "  at {} (no source location recorded)",
                    anchor.identity.def_path.rvs_user_path()
                ));
                continue;
            }
            for location in &locations {
                rvs_collect_location_snippet_BIMS(
                    location,
                    &mut file_cache,
                    &mut located,
                    &mut fallback_lines,
                );
            }
        }
        let snippets = located.iter().map(|piece| {
            Snippet::source(&piece.source)
                .origin(&piece.origin)
                .annotation(level.span(piece.start..piece.end).label(&piece.byte_label))
        });
        let message = level
            .title(&emission.message)
            .id(emission.lint.rvs_as_str())
            .snippets(snippets);
        writeln!(rendered.output, "{}", renderer.render(message))
            .expect("never: writing to String cannot fail");
        for fallback_line in fallback_lines {
            writeln!(rendered.output, "{fallback_line}")
                .expect("never: writing to String cannot fail");
        }
    }
    rendered
}

/// Records the snippet for one resolvable location, or a fallback location
/// line when the file cannot be rendered (missing, non-UTF-8, or offsets
/// outside the content).
fn rvs_collect_location_snippet_BIMS(
    location: &DiagnosticLocation,
    file_cache: &mut BTreeMap<PathBuf, Option<String>>,
    located: &mut Vec<LocatedSnippet>,
    fallback_lines: &mut Vec<String>,
) {
    if !file_cache.contains_key(&location.file) {
        let content = std::fs::read_to_string(&location.file).ok();
        file_cache.insert(location.file.clone(), content);
    }
    let source = file_cache
        .get(&location.file)
        .and_then(Option::as_deref)
        .unwrap_or_default();
    let start = usize::try_from(location.start).unwrap_or(usize::MAX);
    let end = usize::try_from(location.end).unwrap_or(usize::MAX);
    let renderable = start <= end
        && end <= source.len()
        && source.is_char_boundary(start)
        && source.is_char_boundary(end);
    if file_cache.get(&location.file).is_some_and(Option::is_none) || !renderable {
        fallback_lines.push(format!(
            "  --> {} (bytes {}..{})",
            location.file.display(),
            location.start,
            location.end
        ));
        return;
    }
    located.push(LocatedSnippet {
        source: source.to_string(),
        origin: location.file.display().to_string(),
        byte_label: format!("bytes {}..{}", location.start, location.end),
        start,
        end,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{FnNode, FnSource, FunctionIdentity};
    use crate::offline_caps::{OfflineCapsEmissionAnchor, OfflineCapsLint};
    use crate::symbols::DefPath;
    use crate::test_support::{rvs_make_temp_dir_BIST, rvs_snapshot_BIS};
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
        let dir = rvs_make_temp_dir_BIST("graph-render");
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
        assert!(rendered.output.contains("bytes 19..27"));
        assert!(
            rendered
                .output
                .contains("at demo::rvs_sourceless (no source location recorded)")
        );
        assert_eq!(rendered.error_count, 1);
        assert_eq!(rendered.warning_count, 1);
    }

    #[test]
    fn test_20260901_renders_multiline_span_with_carets() {
        let dir = rvs_make_temp_dir_BIST("graph-render-multiline");
        std::fs::create_dir_all(dir.join("src")).expect("never: fixture dir creates");
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn rvs_two() -> u32 {\n    1 +\n    2\n}\n",
        )
        .expect("never: fixture file writes");
        let content = std::fs::read(dir.join("src/lib.rs")).expect("never: fixture reads back");
        let start = content
            .windows(7)
            .position(|window| window == b"rvs_two")
            .expect("never: fixture contains the ident");
        // Span crosses a line break: the renderer underlines both lines.
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 3;
        node.sources = BTreeSet::from([FnSource::rvs_new_relative(
            std::path::PathBuf::from("src/lib.rs"),
            dir.to_path_buf(),
            u32::try_from(start).expect("never: offset fits u32"),
            u32::try_from(start + 20).expect("never: offset fits u32"),
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_two"), node);
        let emission = OfflineCapsEmission {
            lint: OfflineCapsLint::ContractMismatch,
            severity: OfflineCapsSeverity::Error,
            span_anchors: BTreeSet::from([OfflineCapsEmissionAnchor {
                identity: FunctionIdentity {
                    crate_id: 3,
                    def_path: DefPath::from("demo::rvs_two"),
                },
                call_site: None,
            }]),
            message: "multiline span must underline every covered line".to_string(),
        };

        let rendered = rvs_render_graph_emissions_BIS(&graph, &[emission]);
        let output = rendered
            .output
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260901_renders_multiline_span_with_carets", &output);

        assert!(rendered.output.contains("pub fn rvs_two() -> u32 {"));
        assert!(rendered.output.contains("    1 +"));
        assert_eq!(rendered.error_count, 1);
    }

    #[test]
    fn test_20260901_falls_back_for_missing_and_non_utf8_sources() {
        let dir = rvs_make_temp_dir_BIST("graph-render-fallback");
        std::fs::create_dir_all(dir.join("src")).expect("never: fixture dir creates");
        std::fs::write(dir.join("src/lib.rs"), b"pub fn \xff\xfe broken() {}\n")
            .expect("never: fixture file writes");
        let content = std::fs::read(dir.join("src/lib.rs")).expect("never: fixture reads back");
        let start = content
            .windows(6)
            .position(|window| window == b"broken")
            .expect("never: fixture contains the ident");
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 4;
        node.sources = BTreeSet::from([FnSource::rvs_new_relative(
            std::path::PathBuf::from("src/lib.rs"),
            dir.to_path_buf(),
            u32::try_from(start).expect("never: offset fits u32"),
            u32::try_from(start + 6).expect("never: offset fits u32"),
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_non_utf8"), node);
        let mut missing_node = FnNode::default();
        missing_node.crate_id = 5;
        missing_node.sources = BTreeSet::from([FnSource::rvs_new_relative(
            std::path::PathBuf::from("src/gone.rs"),
            dir.to_path_buf(),
            0,
            3,
        )]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_missing"), missing_node);
        let emission = OfflineCapsEmission {
            lint: OfflineCapsLint::ContractMismatch,
            severity: OfflineCapsSeverity::Error,
            span_anchors: BTreeSet::from([
                OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 4,
                        def_path: DefPath::from("demo::rvs_non_utf8"),
                    },
                    call_site: None,
                },
                OfflineCapsEmissionAnchor {
                    identity: FunctionIdentity {
                        crate_id: 5,
                        def_path: DefPath::from("demo::rvs_missing"),
                    },
                    call_site: None,
                },
            ]),
            message: "unrenderable sources degrade to location lines".to_string(),
        };

        let rendered = rvs_render_graph_emissions_BIS(&graph, &[emission]);
        let output = rendered
            .output
            .replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260901_falls_back_for_missing_and_non_utf8_sources",
            &output,
        );

        assert!(rendered.output.contains("src/gone.rs"));
        assert!(!rendered.output.contains('^'));
    }
}
