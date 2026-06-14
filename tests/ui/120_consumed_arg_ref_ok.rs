// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// # Panics
fn rvs_parse_BIPS(data: &str) -> Result<(), String> {
    Err("bad".to_string())
}

#[test]
fn test_20260612_consumed_arg_ref_ok() {
    rvs_parse_BIPS("test").unwrap_err();
}
