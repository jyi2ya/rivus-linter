// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BI() {
    let _ = 1;
}

fn rvs_outer() {
    let _outer = || {
        let inner = || rvs_inner_BI();
        inner();
    };
}

#[test]
fn test_20260801_nested_closure_body_propagates_caps() {
    rvs_outer();
}
