// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_greet(name: &str) -> String {
    format!("hello {}", name)
}

#[test]
fn test_20260612_assert_non_numeric_exempt_ok() {
    rvs_greet("world");
}
