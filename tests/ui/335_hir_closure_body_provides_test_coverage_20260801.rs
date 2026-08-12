// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_target() -> usize {
    1
}

#[test]
fn test_20260801_hir_closure_body_provides_test_coverage() {
    let _never_called = || rvs_target();
}
