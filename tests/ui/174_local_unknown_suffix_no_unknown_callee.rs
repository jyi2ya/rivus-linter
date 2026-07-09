#![allow(non_snake_case)]

fn rvs_dep_Z() -> i32 {
    1
}

fn rvs_call_dep() -> i32 {
    rvs_dep_Z()
}
