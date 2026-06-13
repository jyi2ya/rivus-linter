// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_add(a: i32, b: i32) -> i32 {
    debug_assert!(a >= 0);
    debug_assert!(b >= 0);
    a + b
}

#[test]
fn test_20260612_assert_eq_macro_call() {
    assert_eq!(rvs_add(1, 2), 3);
}
