// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_div(a: i32, b: i32) -> i32 {
    debug_assert!(a >= 0);
    debug_assert!(b != 0);
    a / b
}

#[test]
fn test_20260612_assert_all_covered() {
    rvs_div(10, 2);
}
