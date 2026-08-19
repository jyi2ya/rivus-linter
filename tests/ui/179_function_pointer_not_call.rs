// check-pass
// A function pointer reference creates a weak call edge, so the caller
// must declare the propagated capabilities.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]

fn rvs_env_BS() {
    let _ = std::env::var("HOME");
}

fn rvs_take_pointer_BS() {
    let _function: fn() = rvs_env_BS;
}
