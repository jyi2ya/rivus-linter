// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_empty_fn)]

mod a {
    pub(super) fn rvs_same() {}
}

mod b {
    pub(super) fn rvs_same() {}
}

const _: fn() = b::rvs_same;

#[test]
fn test_20260711_calls_only_a() {
    a::rvs_same();
}
