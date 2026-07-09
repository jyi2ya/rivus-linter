// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_compute(x: i32) -> i32 {
    let _check = || debug_assert!(x > 0);
    x + 1
}

#[test]
fn test_20260705_debug_assert_in_closure_not_param_contract() {
    rvs_compute(1);
}
