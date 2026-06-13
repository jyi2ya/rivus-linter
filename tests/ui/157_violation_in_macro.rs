// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_pure_fn() {
    let _ = format!("calling {}", std::fs::read_to_string("x").unwrap());
}

#[test]
fn test_20260612_violation_in_macro() {
    rvs_pure_fn();
}
