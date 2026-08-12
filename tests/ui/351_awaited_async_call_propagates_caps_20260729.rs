// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

async fn rvs_effect_ABI() {
    let _ = 1;
}

async fn rvs_caller_A() {
    rvs_effect_ABI().await;
}

#[test]
fn test_20260729_awaited_async_call_propagates_caps() {
    let _future = rvs_caller_A();
}
