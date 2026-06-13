// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// A function with P suffix.
///
/// # Panics
///
/// Never panics.
fn rvs_foo_P() {
    let _ = 42;
}

#[test]
fn test_20260612_suffix_no_duplicate_ok() {
    rvs_foo_P();
}
