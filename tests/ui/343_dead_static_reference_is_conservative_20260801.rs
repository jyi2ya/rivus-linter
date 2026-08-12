// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

static VALUE: usize = 1;

fn rvs_outer() {
    let _never_called = || VALUE;
}

#[test]
fn test_20260801_dead_static_reference_is_conservative() {
    rvs_outer();
}
