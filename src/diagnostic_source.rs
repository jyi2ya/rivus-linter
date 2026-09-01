//! Pure resolution of graph-diagnostic anchors to source locations.
//!
//! The resolver answers "where in the real sources does this anchor live"
//! from the merged function graph alone. Rendering is a separate
//! environment concern; this module never touches the file system.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::artifacts::{CallSiteIdentity, CallSiteSource, FnGraph, FnSource, FunctionIdentity};

/// A resolved source location for a graph-diagnostic anchor. `file` is
/// absolute whenever the artifact recorded a base for a relative source
/// name; sources that cannot be anchored to an absolute file are skipped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DiagnosticLocation {
    pub(crate) file: PathBuf,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// Resolves a node anchor onto every recorded source of the defining
/// function. The anchor must match the merged node's full
/// `FunctionIdentity` (def path and stable crate id); a mismatch has no
/// location — the renderer then presents the diagnostic by def path
/// instead of anchoring onto an unrelated node. Canonical-equal sources
/// (absolute spelling vs base-joined relative spelling) collapse; the
/// merged graph keeps the union of sources across Cargo targets.
pub(crate) fn rvs_resolve_node_anchor(
    graph: &FnGraph,
    identity: &FunctionIdentity,
) -> Vec<DiagnosticLocation> {
    let Some(node) = graph.rvs_get(identity.def_path.rvs_as_str()) else {
        return Vec::new();
    };
    if node.crate_id != identity.crate_id {
        return Vec::new();
    }
    node.sources
        .iter()
        .filter_map(rvs_location_of_source)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Resolves a call-site anchor. Call sites carry their own recorded
/// location; an anchor without one has no resolvable position and no
/// fallback to the caller's node — mirroring the replay collector's
/// no-fallback semantics.
pub(crate) fn rvs_resolve_call_site_anchor(
    call_site: &CallSiteIdentity,
) -> Vec<DiagnosticLocation> {
    call_site
        .source
        .as_ref()
        .and_then(rvs_location_of_call_site)
        .into_iter()
        .collect()
}

fn rvs_location_of_source(source: &FnSource) -> Option<DiagnosticLocation> {
    let file = rvs_absolute_file(&source.file, source.base.as_deref())?;
    Some(DiagnosticLocation {
        file,
        start: source.name_start,
        end: source.name_end,
    })
}

fn rvs_location_of_call_site(source: &CallSiteSource) -> Option<DiagnosticLocation> {
    let file = rvs_absolute_file(&source.file, source.base.as_deref())?;
    Some(DiagnosticLocation {
        file,
        start: source.start,
        end: source.end,
    })
}

fn rvs_absolute_file(file: &std::path::Path, base: Option<&std::path::Path>) -> Option<PathBuf> {
    if file.is_absolute() {
        return Some(file.to_path_buf());
    }
    let base = base?;
    if !base.is_absolute() {
        return None;
    }
    Some(base.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallSiteIdentity, FnNode};
    use crate::symbols::DefPath;
    use crate::test_support::rvs_snapshot_BIS;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn rvs_identity(path: &str) -> FunctionIdentity {
        FunctionIdentity {
            crate_id: 7,
            def_path: DefPath::from(path),
        }
    }

    #[test]
    fn test_20260901_resolves_absolute_relative_and_skips_baseless() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.sources = BTreeSet::from([
            FnSource::rvs_new(PathBuf::from("/abs/src/a.rs"), 1, 5),
            FnSource::rvs_new_relative(
                PathBuf::from("src/b.rs"),
                PathBuf::from("/abs/project"),
                2,
                6,
            ),
            FnSource::rvs_new(PathBuf::from("src/baseless.rs"), 3, 7),
        ]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_alpha"), node);

        let locations = rvs_resolve_node_anchor(&graph, &rvs_identity("demo::rvs_alpha"));
        let output = format!("{locations:#?}\n");
        rvs_snapshot_BIS(
            "test_20260901_resolves_absolute_relative_and_skips_baseless",
            &output,
        );

        assert_eq!(locations.len(), 2);
        // DiagnosticLocation order: file names sort lexographically.
        assert_eq!(locations[0].file, Path::new("/abs/project/src/b.rs"));
        assert_eq!((locations[0].start, locations[0].end), (2, 6));
        assert_eq!(locations[1].file, Path::new("/abs/src/a.rs"));
        assert_eq!((locations[1].start, locations[1].end), (1, 5));
    }

    #[test]
    fn test_20260901_node_anchor_requires_exact_identity() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.sources =
            BTreeSet::from([FnSource::rvs_new(PathBuf::from("/abs/src/lib.rs"), 10, 19)]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_alpha"), node);

        let exact = rvs_resolve_node_anchor(&graph, &rvs_identity("demo::rvs_alpha"));
        let stale = rvs_resolve_node_anchor(
            &graph,
            &FunctionIdentity {
                crate_id: 99,
                def_path: DefPath::from("demo::rvs_alpha"),
            },
        );
        let output = format!("exact={exact:#?}\nstale={stale:#?}\n");
        rvs_snapshot_BIS("test_20260901_node_anchor_requires_exact_identity", &output);

        assert_eq!(exact.len(), 1, "exact identity must resolve");
        assert!(
            stale.is_empty(),
            "a crate-id mismatch must not anchor onto the same def path"
        );
    }

    #[test]
    fn test_20260901_canonical_duplicate_sources_deduplicate() {
        let mut graph = FnGraph::rvs_new();
        let mut node = FnNode::default();
        node.crate_id = 7;
        node.sources = BTreeSet::from([
            // Absolute spelling and base-joined relative spelling of the
            // same location must collapse into one candidate.
            FnSource::rvs_new(PathBuf::from("/abs/project/src/lib.rs"), 10, 19),
            FnSource::rvs_new_relative(
                PathBuf::from("src/lib.rs"),
                PathBuf::from("/abs/project"),
                10,
                19,
            ),
            // A different range in the same file stays a distinct
            // candidate.
            FnSource::rvs_new(PathBuf::from("/abs/project/src/lib.rs"), 40, 49),
        ]);
        graph.rvs_insert_M(DefPath::from("demo::rvs_alpha"), node);

        let locations = rvs_resolve_node_anchor(&graph, &rvs_identity("demo::rvs_alpha"));
        let output = format!("{locations:#?}\n");
        rvs_snapshot_BIS(
            "test_20260901_canonical_duplicate_sources_deduplicate",
            &output,
        );

        assert_eq!(
            locations,
            vec![
                DiagnosticLocation {
                    file: PathBuf::from("/abs/project/src/lib.rs"),
                    start: 10,
                    end: 19,
                },
                DiagnosticLocation {
                    file: PathBuf::from("/abs/project/src/lib.rs"),
                    start: 40,
                    end: 49,
                },
            ],
            "canonical-equal sources collapse; different ranges stay distinct"
        );
    }

    #[test]
    fn test_20260901_missing_node_and_missing_call_site_source() {
        let graph = FnGraph::rvs_new();
        let node_locations = rvs_resolve_node_anchor(&graph, &rvs_identity("demo::rvs_missing"));
        let call_site = CallSiteIdentity {
            callee: rvs_identity("demo::rvs_callee"),
            occurrence: 0,
            source: None,
        };
        let call_locations = rvs_resolve_call_site_anchor(&call_site);
        let output = format!("node={node_locations:#?}\ncall={call_locations:#?}\n");
        rvs_snapshot_BIS(
            "test_20260901_missing_node_and_missing_call_site_source",
            &output,
        );

        assert!(node_locations.is_empty());
        assert!(call_locations.is_empty());
    }

    #[test]
    fn test_20260901_call_site_source_resolves_without_fallback() {
        let call_site = CallSiteIdentity {
            callee: rvs_identity("demo::rvs_callee"),
            occurrence: 2,
            source: Some(crate::artifacts::CallSiteSource::rvs_new_relative(
                PathBuf::from("src/lib.rs"),
                PathBuf::from("/abs/project"),
                10,
                24,
            )),
        };
        let locations = rvs_resolve_call_site_anchor(&call_site);
        let output = format!("{locations:#?}\n");
        rvs_snapshot_BIS(
            "test_20260901_call_site_source_resolves_without_fallback",
            &output,
        );

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].file, Path::new("/abs/project/src/lib.rs"));
        assert_eq!((locations[0].start, locations[0].end), (10, 24));
    }
}
