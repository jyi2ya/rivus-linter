use rustc_lexer::{FrontmatterAllowed, TokenKind};
use rustc_lint::LateContext;
use rustc_span::Span;

use super::super::RVS_TODO_COMMENT;
use super::super::msg::rvs_emit_span_lint_S;

/// Check source span for `// TODO` or `// FIXME` comments.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, span: Span) {
    let source_map = cx.tcx.sess.source_map();
    if let Ok(src) = source_map.span_to_snippet(span)
        && rvs_source_has_marker_comment(&src)
    {
        rvs_emit_span_lint_S(cx, RVS_TODO_COMMENT, span, "TODO/FIXME comment found");
    }
}

fn rvs_source_has_marker_comment(source: &str) -> bool {
    let mut offset = 0usize;
    for token in rustc_lexer::tokenize(source, FrontmatterAllowed::No) {
        let Ok(token_len) = usize::try_from(token.len) else {
            return false;
        };
        let Some(end) = offset.checked_add(token_len) else {
            return false;
        };
        let is_comment = matches!(
            token.kind,
            TokenKind::LineComment { .. } | TokenKind::BlockComment { .. }
        );
        if is_comment && source.get(offset..end).is_some_and(rvs_contains_marker) {
            return true;
        }
        offset = end;
    }
    false
}

fn rvs_contains_marker(comment: &str) -> bool {
    comment
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| token.eq_ignore_ascii_case("todo") || token.eq_ignore_ascii_case("fixme"))
}

#[cfg(test)]
mod tests {
    use super::{rvs_contains_marker, rvs_source_has_marker_comment};
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260714_todo_marker_requires_identifier_boundaries() {
        let cases = [
            ("// TODO: repair", true),
            ("/* fixme later */", true),
            ("// autodoc output", false),
            ("/* prefixfixme text */", false),
            ("// TODO_item", false),
        ];
        let output = cases
            .iter()
            .map(|(comment, expected)| {
                format!(
                    "{comment}: actual={}, expected={expected}",
                    rvs_contains_marker(comment)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260714_todo_marker_requires_identifier_boundaries",
            &(output + "\n"),
        );

        for (comment, expected) in cases {
            assert_eq!(rvs_contains_marker(comment), expected);
        }
    }

    #[test]
    fn test_20260714_todo_scanner_uses_comment_tokens() {
        let cases = [
            ("let _ = 1; // TODO: trailing", true),
            ("/* line one\n * FIXME: continuation\n */", true),
            ("r#\"// TODO: serialized data\"#", false),
            ("\"/* FIXME: serialized data */\"", false),
        ];
        let output = cases
            .iter()
            .map(|(source, expected)| {
                format!(
                    "{source:?}: actual={}, expected={expected}",
                    rvs_source_has_marker_comment(source)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        rvs_snapshot_BIS("test_20260714_todo_scanner_uses_comment_tokens", &output);

        for (source, expected) in cases {
            assert_eq!(rvs_source_has_marker_comment(source), expected, "{source}");
        }
    }
}
