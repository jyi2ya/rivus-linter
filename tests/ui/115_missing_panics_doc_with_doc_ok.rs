// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// Does a division.
///
/// # Panics
///
/// Panics if b is zero.
fn rvs_divide(a: i32, b: i32) -> i32 {
    debug_assert!(a >= 0);
    debug_assert!(b != 0);
    a / b
}

#[test]
fn test_20260612_missing_panics_doc_with_doc_ok() {
    rvs_divide(10, 2);
}
