use std::collections::HashSet;

use rustc_hir::{Item, UseKind, UsePath, def::Res};
use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::msg::rvs_emit_span_lint_S;
use super::super::{RVS_BANNED_IMPORT, RVS_TESTS_IMPORT, RVS_WILDCARD_IMPORT};

fn rvs_emit_banned_crate_S(cx: &LateContext<'_>, span: Span, crate_name: &str) {
    rvs_emit_span_lint_S(
        cx,
        RVS_BANNED_IMPORT,
        span,
        format!("banned import: {crate_name}"),
    );
}

fn rvs_span_snippet_has_use_keyword(snippet: &str) -> bool {
    let trimmed = snippet.trim_start();
    trimmed.starts_with("use ")
        || trimmed.starts_with("use\n")
        || trimmed.starts_with("pub ")
        || trimmed.starts_with("pub(")
}

/// Check an `extern crate` item using the resolved crate identity, not its alias.
pub(crate) fn rvs_check_extern_crate_S(cx: &LateContext<'_>, item: &Item<'_>) {
    let Some(crate_num) = cx.tcx.extern_mod_stmt_cnum(item.owner_id.def_id) else {
        return;
    };
    let crate_name_symbol = cx.tcx.crate_name(crate_num);
    let crate_name = crate_name_symbol.as_str();
    if matches!(crate_name, "anyhow" | "eyre" | "color_eyre" | "thiserror") {
        rvs_emit_banned_crate_S(cx, item.span, crate_name);
    }
}

/// Check `use` items for banned crates (anyhow/eyre/color_eyre/thiserror),
/// wildcard imports (`use xxx::*`), and imports of `tests`-module symbols
/// from outside the `tests` module.
pub(crate) fn rvs_check_item_BMS<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    path: &'tcx UsePath<'tcx>,
    use_kind: UseKind,
    seen_statements: &mut HashSet<(rustc_span::StableSourceFileId, u32, String)>,
) {
    let owner_def_id = item.owner_id.def_id.to_def_id();
    let current_path = cx.tcx.def_path_str(owner_def_id);
    let importer_in_tests = rvs_path_has_tests_segment(&current_path);
    for resolution in path.res.present_items() {
        if let Res::Def(_, def_id) = resolution {
            if !importer_in_tests && def_id.is_local() {
                let def_path = cx.tcx.def_path_str(def_id);
                if rvs_path_has_tests_segment(&def_path) {
                    rvs_emit_span_lint_S(
                        cx,
                        RVS_TESTS_IMPORT,
                        item.span,
                        format!("import of tests-module symbol '{def_path}' from non-test code"),
                    );
                }
            }
            if def_id.is_local() {
                continue;
            }
            let crate_name_symbol = cx.tcx.crate_name(def_id.krate);
            let crate_name = crate_name_symbol.as_str();
            if !matches!(crate_name, "anyhow" | "eyre" | "color_eyre" | "thiserror") {
                continue;
            }
            let source_map = cx.tcx.sess.source_map();
            let statement_span = if item.span.from_expansion() {
                item.span.source_callsite()
            } else if source_map
                .span_to_snippet(item.span)
                .is_ok_and(|snippet| rvs_span_snippet_has_use_keyword(&snippet))
            {
                item.span
            } else {
                source_map
                    .span_extend_to_prev_str(item.span, "use", true, false)
                    .unwrap_or(item.span)
            };
            let source_file = source_map.lookup_source_file(statement_span.lo());
            let should_emit = seen_statements.insert((
                source_file.stable_id,
                statement_span.lo().0,
                crate_name.to_string(),
            ));
            if should_emit {
                rvs_emit_banned_crate_S(cx, statement_span, crate_name);
            }
            break;
        }
    }
    let ps: Vec<_> = path
        .segments
        .iter()
        .map(|s| s.ident.name.as_str())
        .collect();
    if matches!(use_kind, UseKind::Glob) {
        if ps.last().map(|s| *s) == Some("prelude") {
            // prelude ok
        } else if ps.as_slice() == ["super"] {
            // super ok
        } else {
            let full = format!("{}::*", ps.join("::"));
            rvs_emit_span_lint_S(
                cx,
                RVS_WILDCARD_IMPORT,
                item.span,
                format!("wildcard import: {full}"),
            );
        }
    }
}

fn rvs_path_has_tests_segment(path: &str) -> bool {
    path.split("::").any(|segment| segment == "tests")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260714_import_span_keyword_detection_table() {
        let cases = [
            ("use anyhow::Result;", true),
            ("  pub use eyre::Report;", true),
            ("pub(crate) use thiserror::Error;", true),
            ("anyhow::{Context, Result}", false),
        ];
        let output = cases
            .iter()
            .map(|(snippet, expected)| {
                format!(
                    "{snippet:?}: actual={}, expected={expected}",
                    rvs_span_snippet_has_use_keyword(snippet)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260714_import_span_keyword_detection_table", &output);

        for (snippet, expected) in cases {
            assert_eq!(rvs_span_snippet_has_use_keyword(snippet), expected);
        }
    }
}
