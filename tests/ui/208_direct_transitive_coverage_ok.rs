// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_target() -> i32 {
    1
}

fn rvs_helper() -> i32 {
    rvs_target()
}

#[test]
fn test_20260714_direct_transitive_coverage_ok() {
    assert_eq!(rvs_helper(), 1);
}
