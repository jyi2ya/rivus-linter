// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_foo(x: i32) -> i32 {
    if x > 0 {
        debug_assert!(x > 0);
    }
    x
}

#[test]
fn test_20260612_assert_nested_block_ok() {
    rvs_foo(5);
}
