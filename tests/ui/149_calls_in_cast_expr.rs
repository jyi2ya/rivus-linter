// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_inner_ABI() -> i32 {
    42
}

fn rvs_outer() {
    let _ = rvs_inner_ABI() as i64;
}

#[test]
fn test_20260612_calls_in_cast_expr() {
    rvs_outer();
}
