// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_blocking_BI() {
    let _ = 1;
}

fn rvs_wrapped() {
    ({ rvs_blocking_BI })();
}

#[test]
fn test_20260714_block_wrapped_callee() {
    rvs_wrapped();
}
