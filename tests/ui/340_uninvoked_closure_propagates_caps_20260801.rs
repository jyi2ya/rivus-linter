// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_outer_BIS() {
    let _never_called = || rvs_inner_BIS();
}

#[test]
fn test_20260801_uninvoked_closure_body_propagates_caps() {
    rvs_outer_BIS();
}
