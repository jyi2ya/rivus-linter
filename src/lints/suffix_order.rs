use rustc_lint::{LateContext, LintContext};
use rustc_span::Span;

use super::msg::Msg;
use super::{RVS_DUPLICATE_SUFFIX, RVS_NON_ALPHABETICAL_SUFFIX, RVS_UNKNOWN_SUFFIX_LETTER};

/// Check suffix ordering, duplicates, and unknown letters.
pub(crate) fn rvs_check_fn_S(cx: &LateContext<'_>, span: Span, raw_suffix: &str) {
    let raw = raw_suffix;
    if raw.is_empty() {
        return;
    }
    let mut cv: Vec<char> = raw.chars().collect();
    cv.sort();
    let sorted: String = cv.into_iter().collect();
    if raw != sorted {
        cx.emit_span_lint(
            RVS_NON_ALPHABETICAL_SUFFIX,
            span,
            Msg::new(span, "suffix not alphabetical"),
        );
    }
    let mut seen = std::collections::HashSet::new();
    for c in raw.chars() {
        if !seen.insert(c) {
            cx.emit_span_lint(
                RVS_DUPLICATE_SUFFIX,
                span,
                Msg::new(span, format!("duplicate '{c}'")),
            );
            break;
        }
    }
    let unk = crate::capability::rvs_extract_unknown_suffix_letters(raw);
    if !unk.is_empty() {
        cx.emit_span_lint(
            RVS_UNKNOWN_SUFFIX_LETTER,
            span,
            Msg::new(
                span,
                format!(
                    "unknown letters: {}",
                    unk.iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        );
    }
}
