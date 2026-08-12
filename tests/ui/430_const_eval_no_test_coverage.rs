// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

const fn rvs_const_target() -> usize {
    1
}

fn rvs_runtime_caller() {
    let _ = const { rvs_const_target() };
}

#[test]
fn test_20260811_const_eval_is_not_runtime_coverage() {
    rvs_runtime_caller();
}
