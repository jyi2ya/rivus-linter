// check-pass
#![allow(non_snake_case)]

/// Does something.
///
/// # Panics
///
/// Panics if msg is empty.
fn rvs_bail_P(msg: &str) {
    panic!("{}", msg);
}
