#![allow(non_snake_case)]

fn rvs_get_value(x: Result<i32, String>) -> i32 {
    x.expect("must succeed")
}
