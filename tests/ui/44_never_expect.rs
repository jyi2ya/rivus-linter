#![allow(non_snake_case)]

fn rvs_never_expect() {
    let _: i32 = "42".parse().expect("never: parsed from literal");
}

fn rvs_normal_expect() {
    let _: i32 = "42".parse().expect("might fail");
}
