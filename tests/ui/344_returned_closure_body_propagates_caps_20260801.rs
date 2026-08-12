// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BI() {
    let _ = 1;
}

fn rvs_build_deferred() -> impl FnOnce() {
    move || rvs_inner_BI()
}

fn rvs_outer() {
    let _deferred = rvs_build_deferred();
}

#[test]
fn test_20260801_returned_closure_body_propagates_caps() {
    rvs_outer();
}
