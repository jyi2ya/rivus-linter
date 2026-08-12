// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_effect_B() {
    let _ = 1;
}

const CONST_HOOK: fn() = rvs_effect_B;
static STATIC_HOOK: fn() = rvs_effect_B;

trait Hook {
    const ASSOCIATED_HOOK: fn();
}

struct Hooks;

impl Hook for Hooks {
    const ASSOCIATED_HOOK: fn() = rvs_effect_B;
}

fn rvs_runtime_callable_values_S() {
    CONST_HOOK();
    STATIC_HOOK();
    Hooks::ASSOCIATED_HOOK();
}

#[test]
fn test_20260811_runtime_const_static_callable_warns() {
    rvs_runtime_callable_values_S();
}
