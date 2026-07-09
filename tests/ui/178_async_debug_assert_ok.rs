// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

async fn rvs_compute_A(x: i32) -> i32 {
    debug_assert!(x > 0);
    x + 1
}

#[test]
fn test_20260708_async_debug_assert_ok() {
    std::mem::drop(rvs_compute_A(1));
}
