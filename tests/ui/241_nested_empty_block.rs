#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, reason = "fixture isolates empty-body wording")]

fn rvs_empty() {
    {}
}
