// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BI() {
    let _ = 1;
}

async fn rvs_outer() {
    async {
        rvs_inner_BI();
    }
    .await;
}

#[test]
fn test_20260716_awaited_async_block_propagates_caps() {
    let _future = rvs_outer();
}
