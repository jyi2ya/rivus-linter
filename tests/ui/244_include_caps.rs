#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, reason = "fixture isolates include source tracking")]

fn rvs_blocking_BI() {
    let _ = 1;
}

include!("244_include_caps.inc");
