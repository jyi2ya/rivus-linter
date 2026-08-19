// check-pass
// Tests that a function passed as a callback argument creates a weak call
// edge, propagating capabilities from the referenced function to the caller.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn, rivus::rvs_missing_doc)]
#![allow(rivus::rvs_unsupported_indirect_call)]

fn rvs_callback_BS() {
    let _ = std::env::var("HOME");
}

fn rvs_run_callback<F: Fn()>(f: F) {
    f();
}

fn rvs_caller_BS() {
    rvs_run_callback(rvs_callback_BS);
}
