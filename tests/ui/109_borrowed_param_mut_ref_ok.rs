// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// # Panics
fn rvs_append_M(s: &mut String) {
    s.push_str("x");
}

#[test]
fn test_20260612_borrowed_param_mut_ref_ok() {
    let mut s = String::new();
    rvs_append_M(&mut s);
}
