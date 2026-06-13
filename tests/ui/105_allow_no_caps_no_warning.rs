// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_pure() {
    let _ = 42;
}

#[test]
fn test_20260612_allow_no_caps_no_warning() {
    rvs_pure();
}
