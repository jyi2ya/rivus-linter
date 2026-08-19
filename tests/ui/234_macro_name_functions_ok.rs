// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_non_rvs_fn)]

static CALLS: u8 = 0;

fn todo() {
    let _ = 1;
}

fn debug_assert() {
    let _ = 1;
}

fn rvs_call_named_functions_S() -> u8 {
    todo();
    debug_assert();
    CALLS
}
