// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_comments_without_markers() {
    // Autodoc output does not request follow-up work.
    /* Prefixfixme text is also ordinary prose. */
    let _ = r#"
// TODO: serialized data, not a source comment.
"#;
    let _ = 42;
}

#[test]
fn test_20260714_todo_substring_ok() {
    rvs_comments_without_markers();
}
