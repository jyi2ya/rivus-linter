// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// Gets a value.
fn rvs_get(x: Option<i32>) -> i32 {
    x.unwrap_or(0)
}

#[test]
fn test_20260617_get_ok() {
    assert_eq!(rvs_get(Some(42)), 42);
}
