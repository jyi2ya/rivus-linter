// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BI() {
    let _ = 1;
}

fn rvs_outer() {
    let _never_called = || rvs_inner_BI();
}

#[test]
fn test_20260801_uninvoked_closure_propagates_caps() {
    rvs_outer();
}
