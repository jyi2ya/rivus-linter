// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

macro_rules! todo {
    () => {{ 42 }};
}

fn rvs_value() -> i32 {
    todo!()
}

#[test]
fn test_20260714_custom_stub_macro_ok() {
    assert_eq!(rvs_value(), 42);
}
