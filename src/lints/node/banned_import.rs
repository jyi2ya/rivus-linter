use std::collections::HashSet;

use rustc_hir::{Item, UseKind, UsePath, def::Res};
use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::super::msg::Msg;
use super::super::{RVS_BANNED_IMPORT, RVS_WILDCARD_IMPORT};

fn rvs_emit_banned_crate_S(cx: &LateContext<'_>, span: Span, crate_name: &str) {
    cx.emit_span_lint(
        RVS_BANNED_IMPORT,
        span,
        Msg::rvs_new(span, format!("banned import: {crate_name}")),
    );
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

/// Check `use` items for banned crates (anyhow/eyre/color_eyre/thiserror) and
/// wildcard imports (`use xxx::*`).
pub(crate) fn rvs_check_item_MS<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    path: &'tcx UsePath<'tcx>,
    use_kind: UseKind,
    seen_statements: &mut HashSet<(rustc_span::StableSourceFileId, u32, String)>,
) {
    for resolution in path.res.present_items() {
        if let Res::Def(_, def_id) = resolution {
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
            cx.emit_span_lint(
                RVS_WILDCARD_IMPORT,
                item.span,
                Msg::rvs_new(item.span, format!("wildcard import: {full}")),
            );
        }
    }
}
