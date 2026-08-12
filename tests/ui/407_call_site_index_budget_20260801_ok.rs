// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_leaf() {
    let _ = 1;
}

fn rvs_call_heavy() {
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
    rvs_leaf();
}

#[test]
fn test_20260801_call_site_index_budget() {
    rvs_call_heavy();
}
