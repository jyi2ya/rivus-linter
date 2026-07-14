// check-pass
#![allow(non_snake_case)]

struct Result;

fn rvs_drop_custom_result_S() {
    drop(Result);
}
