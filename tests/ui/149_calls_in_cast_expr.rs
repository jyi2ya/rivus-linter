// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_inner_BIS() -> i32 {
    let _ = std::fs::remove_file("fixture-marker");
    42
}

fn rvs_outer_BIS() {
    let _ = rvs_inner_BIS() as i64;
}

#[test]
fn test_20260612_calls_in_cast_expr() {
    rvs_outer_BIS();
}
