// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_compute(x: i32, name: &str) -> i32 {
    x + 1
}

#[test]
fn test_20260612_assert_numeric_unasserted() {
    rvs_compute(5, "test");
}
