// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_compute(x: i32) -> i32 {
    let original = x;
    let x = "shadow";
    debug_assert!(!x.is_empty());
    original
}

#[test]
fn test_20260714_debug_assert_shadowed_param() {
    assert_eq!(rvs_compute(7), 7);
}
