// check-pass
#![allow(non_snake_case)]

/// Gets a value.
///
/// # Panics
///
/// Panics if x is Err.
fn rvs_get_P(x: Result<i32, String>) -> i32 {
    x.expect("fail")
}
