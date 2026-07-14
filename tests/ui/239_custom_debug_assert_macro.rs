// compile-flags: --test
#![allow(non_snake_case)]

macro_rules! debug_assert {
    ($condition:expr) => {{ let _ = $condition; }};
}

fn rvs_compute(x: i32) -> i32 {
    debug_assert!(x > 0);
    x
}

#[test]
fn test_20260714_custom_debug_assert_macro() {
    assert_eq!(rvs_compute(7), 7);
}
