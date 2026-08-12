// Tests that unsupported indirect calls in non-rvs free functions
// also emit warnings, since the check is now in the common body pipeline.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]
#![allow(rivus::rvs_missing_doc)]
#![allow(rivus::rvs_non_rvs_fn)]

fn helper(callback: fn()) {
    callback();
    //~^ ERROR: call through function pointer
}
