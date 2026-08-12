// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

async fn rvs_target_A() -> usize {
    7
}

#[test]
fn test_20260801_unawaited_async_call_provides_hir_coverage() {
    let _future = rvs_target_A();
}
