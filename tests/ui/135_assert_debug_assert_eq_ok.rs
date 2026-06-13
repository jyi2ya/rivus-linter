// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_process(count: usize) -> bool {
    debug_assert_eq!(count, 0);
    count == 0
}

#[test]
fn test_20260612_assert_debug_assert_eq_ok() {
    rvs_process(0);
}
