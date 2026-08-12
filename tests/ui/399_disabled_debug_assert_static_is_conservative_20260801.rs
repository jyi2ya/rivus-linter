// compile-flags: --test -Cdebug-assertions=no
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

static VALUE: usize = 7;

fn rvs_value() -> usize {
    debug_assert_eq!(VALUE, 7);
    0
}

#[test]
fn test_20260801_disabled_debug_assert_static_is_conservative() {
    assert_eq!(rvs_value(), 0);
}
