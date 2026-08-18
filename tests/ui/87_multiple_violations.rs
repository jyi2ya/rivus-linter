#![allow(non_snake_case)]

fn rvs_inner_BI() {}
fn rvs_outer() {
    rvs_inner_BI();
}
fn rvs_pure_M() {}
fn rvs_caller() {
    rvs_inner_BI();
    rvs_pure_M();
}
