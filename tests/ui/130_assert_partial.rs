// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_div(a: i32, b: i32) -> i32 {
    debug_assert!(b != 0, "divisor must be non-zero");
    a / b
}

#[test]
fn test_20260612_assert_partial() {
    rvs_div(10, 2);
}
