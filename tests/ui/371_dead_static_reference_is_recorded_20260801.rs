// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

static VALUE: usize = 7;

fn rvs_value() -> usize {
    if false { VALUE } else { 0 }
}

#[test]
fn test_20260801_dead_static_reference_is_recorded() {
    assert_eq!(rvs_value(), 0);
}
