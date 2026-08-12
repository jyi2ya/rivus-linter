// compile-flags: --test
// A function pointer reference creates a weak edge that propagates
// capabilities but does NOT provide test coverage.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_target() {
    let _ = 1;
}

#[test]
fn test_20260811_reference_is_not_coverage() {
    let _pointer: fn() = rvs_target;
}
