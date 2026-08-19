// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_build_deferred_BIS() -> impl FnOnce() {
    move || rvs_inner_BIS()
}

fn rvs_outer_BIS() {
    let _deferred = rvs_build_deferred_BIS();
}

#[test]
fn test_20260801_returned_closure_body_propagates_caps() {
    rvs_outer_BIS();
}
