// Tests that unsupported indirect calls in non-rvs trait impl methods
// also emit warnings, since the check is now in the common body pipeline.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]
#![allow(rivus::rvs_missing_doc)]
#![allow(rivus::rvs_non_rvs_fn)]

trait Runner {
    fn run(&self, callback: fn());
}

struct Worker;

impl Runner for Worker {
    fn run(&self, callback: fn()) {
        callback();
        //~^ ERROR: call through function pointer
    }
}
