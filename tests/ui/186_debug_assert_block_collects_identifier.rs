// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_compute(x: i32) -> i32 {
    debug_assert!({
        let observed = x;
        observed > 0
    });
    x + 1
}

#[test]
fn test_20260711_debug_assert_block_collects_identifier() {
    assert_eq!(rvs_compute(1), 2);
}
