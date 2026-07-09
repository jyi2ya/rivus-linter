#![allow(non_snake_case)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

fn rvs_check_M(n: i32) {
    debug_assert!(n > 0);
}
