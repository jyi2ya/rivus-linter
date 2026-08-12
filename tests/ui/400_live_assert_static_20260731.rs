// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

static VALUE: usize = 7;

fn rvs_value() -> usize {
    assert_eq!(VALUE, 7);
    0
}

#[test]
fn test_20260731_live_assert_static() {
    assert_eq!(rvs_value(), 0);
}
