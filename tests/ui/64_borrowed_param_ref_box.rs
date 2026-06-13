#![allow(non_snake_case)]

pub fn rvs_foo(b: &Box<i32>) -> i32 {
    **b
}
