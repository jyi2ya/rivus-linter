// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_effect_BI() {
    let _ = 1;
}

fn rvs_caller() {
    if false {
        rvs_effect_BI();
    }
}

#[test]
fn test_20260801_dead_call_propagates_caps() {
    rvs_caller();
}
