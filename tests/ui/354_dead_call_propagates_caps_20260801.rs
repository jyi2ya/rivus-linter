// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_effect_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_caller_BIS() {
    if false {
        rvs_effect_BIS();
    }
}

#[test]
fn test_20260801_dead_call_propagates_caps() {
    rvs_caller_BIS();
}
