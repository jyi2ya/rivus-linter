// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_target() -> usize {
    7
}

#[test]
fn test_20260801_constant_dead_call_provides_hir_coverage() {
    if false {
        assert_eq!(rvs_target(), 7);
    }
}
