// check-pass
#![allow(non_snake_case)]

/// Gets a value.
///
/// # Panics
///
/// Panics if x is None.
fn rvs_get_P(x: Option<i32>) -> i32 {
    x.unwrap()
}
