// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_take(s: String) {
    let _ = s;
}

#[test]
fn test_20260612_borrowed_param_owned_ok() {
    rvs_take("hello".to_string());
}
