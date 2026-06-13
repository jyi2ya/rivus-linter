#![allow(non_snake_case)]

fn rvs_inner_ABI() {}
fn rvs_outer() {
    rvs_inner_ABI();
}
fn rvs_pure_M() {}
fn rvs_caller() {
    rvs_inner_ABI();
    rvs_pure_M();
}
