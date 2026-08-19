// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

async fn rvs_outer_BIS() {
    async {
        rvs_inner_BIS();
    }
    .await;
}

#[test]
fn test_20260716_awaited_async_block_propagates_caps() {
    let _future = rvs_outer_BIS();
}
