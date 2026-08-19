// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_check_BIS(x: i32) -> bool {
    debug_assert!(x >= i32::MIN);
    let _ = std::fs::remove_file("fixture-marker");
    x > 0
}

fn rvs_outer_BIS(x: i32) {
    debug_assert!(x >= i32::MIN);
    match x {
        n if rvs_check_BIS(n) => {}
        _ => {}
    }
}

#[test]
fn test_20260612_calls_in_match_guard() {
    rvs_outer_BIS(5);
}
