// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// # Panics
///
/// Panics if value is None.
fn rvs_baz() {
    let _ = Some(42).unwrap();
}

#[test]
fn test_20260612_error_swallow_none_ok() {
    rvs_baz();
}
