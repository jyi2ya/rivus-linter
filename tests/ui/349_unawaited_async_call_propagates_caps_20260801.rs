// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

async fn rvs_effect_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_caller_BIS() {
    let _future = rvs_effect_BIS();
}

#[test]
fn test_20260801_unawaited_async_call_propagates_caps() {
    rvs_caller_BIS();
}
