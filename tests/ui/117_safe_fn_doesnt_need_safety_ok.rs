// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// A safe function does not need Safety doc.
fn rvs_safe() {
    let _ = 42;
}

#[test]
fn test_20260612_safe_fn_doesnt_need_safety_ok() {
    rvs_safe();
}
