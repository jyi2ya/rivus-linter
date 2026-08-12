// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_indirect_call)]

fn rvs_target() -> usize {
    1
}

#[test]
fn test_20260716_invoked_closure_provides_test_coverage() {
    let call = || rvs_target();
    assert_eq!(call(), 1);
}
