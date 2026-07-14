// compile-flags: --test --crate-name=core
#![feature(register_tool)]
#![allow(non_snake_case)]
#![register_tool(rivus)]
#![allow(rivus::rvs_non_rvs_fn)]

macro_rules! debug_assert {
    ($condition:expr) => {{
        let _ = $condition;
    }};
}

macro_rules! todo {
    () => {{
        1u32
    }};
}

fn rvs_custom_core_debug_assert(value: u32) {
    debug_assert!(value > 0);
    //~^ ERROR primitive numeric parameter 'value' must have a debug_assert! precondition
}

fn rvs_custom_core_todo() -> u32 {
    todo!()
}

#[test]
fn test_20260714_custom_core_macros() {
    rvs_custom_core_debug_assert(1);
    assert_eq!(rvs_custom_core_todo(), 1);
}
