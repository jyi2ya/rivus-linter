// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

async fn rvs_effect_ABI() {
    let _ = 1;
}

fn rvs_caller() {
    let _future = rvs_effect_ABI();
}

#[test]
fn test_20260801_unawaited_async_call_propagates_caps() {
    rvs_caller();
}
