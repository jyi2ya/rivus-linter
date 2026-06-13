// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_check_ABI(x: i32) -> bool {
    x > 0
}

fn rvs_outer(x: i32) {
    match x {
        n if rvs_check_ABI(n) => {}
        _ => {}
    }
}

#[test]
fn test_20260612_calls_in_match_guard() {
    rvs_outer(5);
}
