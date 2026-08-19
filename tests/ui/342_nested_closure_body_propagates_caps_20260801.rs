// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_indirect_call)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_outer_BIS() {
    let _outer = || {
        let inner = || rvs_inner_BIS();
        inner();
    };
}

#[test]
fn test_20260801_nested_closure_body_propagates_caps() {
    rvs_outer_BIS();
}
