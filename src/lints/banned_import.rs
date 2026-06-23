use rustc_hir::{Item, UseKind, UsePath};
use rustc_lint::{LateContext, LintContext};

use super::msg::Msg;
use super::{RVS_BANNED_IMPORT, RVS_WILDCARD_IMPORT};

/// Check `use` items for banned crates (anyhow/eyre/color_eyre) and
/// wildcard imports (`use xxx::*`).
pub(crate) fn rvs_check_item_S<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx Item<'tcx>,
    path: &'tcx UsePath<'tcx>,
    use_kind: UseKind,
) {
    for seg in path.segments {
        let n = seg.ident.name.as_str();
        if n == "anyhow" || n == "eyre" || n == "color_eyre" {
            cx.emit_span_lint(
                RVS_BANNED_IMPORT,
                item.span,
                Msg::new(item.span, format!("banned import: {n}")),
            );
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
        } else if ps.len() == 1 && ps[0] == "super" {
            // super ok
        } else {
            let full = format!("{}::*", ps.join("::"));
            cx.emit_span_lint(
                RVS_WILDCARD_IMPORT,
                item.span,
                Msg::new(item.span, format!("wildcard import: {full}")),
            );
        }
    }
}
