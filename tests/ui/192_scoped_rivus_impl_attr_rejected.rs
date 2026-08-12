// compile-flags: --test
#![allow(non_snake_case)]
#![feature(register_tool)]
#![register_tool(rivus)]

#[derive(Debug)]
struct Guard;

impl Guard {
    #[allow(rivus::rvs_non_rvs_fn)]
    fn catch_unwind(&self) {}
}

fn rvs_run() {
    Guard.catch_unwind();
}

#[test]
fn test_20260714_custom_catch_unwind_method_ok() {
    rvs_run();
}
