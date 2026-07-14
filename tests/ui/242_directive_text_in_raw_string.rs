// compile-flags: --test
#![allow(non_snake_case)]

const HELP: &str = r#"
// check-pass
"#;

fn rvs_compute(x: i32) -> i32 {
    let _ = HELP;
    x
}

#[test]
fn test_20260714_directive_text_in_raw_string() {
    assert_eq!(rvs_compute(7), 7);
}
