// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_foo(s: &str) -> usize {
    s.len()
}

#[test]
fn test_20260612_borrowed_param_str_ok() {
    rvs_foo("hello");
}
