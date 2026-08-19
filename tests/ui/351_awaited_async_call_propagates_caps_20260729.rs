// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

async fn rvs_effect_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

async fn rvs_caller_BIS() {
    rvs_effect_BIS().await;
}

#[test]
fn test_20260729_awaited_async_call_propagates_caps() {
    let _future = rvs_caller_BIS();
}
