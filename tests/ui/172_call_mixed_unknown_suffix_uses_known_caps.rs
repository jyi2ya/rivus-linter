// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unknown_suffix_letter)]

fn rvs_send_EIS() {
    let _ = 1;
}

fn rvs_outer() {
    rvs_send_EIS();
}

#[test]
fn test_20260705_call_mixed_unknown_suffix_uses_known_caps() {
    rvs_outer();
}
