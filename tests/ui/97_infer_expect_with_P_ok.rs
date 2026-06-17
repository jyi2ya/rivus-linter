// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// Gets a value.
fn rvs_get(x: Result<i32, String>) -> i32 {
    x.unwrap_or(0)
}

#[test]
fn test_20260617_get_result_ok() {
    assert_eq!(rvs_get(Ok(42)), 42);
}
