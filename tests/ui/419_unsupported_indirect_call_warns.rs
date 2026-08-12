// Tests that calls through function pointer parameters and generic callable
// parameters produce RVS_UNSUPPORTED_INDIRECT_CALL warnings.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]
#![allow(rivus::rvs_missing_doc)]

fn rvs_indirect_fn_pointer(callback: fn()) {
    callback();
    //~^ ERROR: call through function pointer
}

fn rvs_indirect_generic<F: Fn()>(callback: F) {
    callback();
    //~^ ERROR: call through function pointer
}
