// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// A dangerous function.
///
/// # Safety
///
/// Caller must ensure ptr is valid.
unsafe fn rvs_dangerous_U() {
    let _ = 42;
}

#[test]
fn test_20260612_unsafe_fn_with_safety_doc_ok() {
    unsafe { rvs_dangerous_U() };
}
