// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]

fn rvs_io_BI() {
    let _ = 1;
}

fn rvs_take_pointer() {
    let _function: fn() = rvs_io_BI;
}
