// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_pure() -> i32 {
    42
}

#[test]
fn test_20260612_assert_no_params_ok() {
    rvs_pure();
}
