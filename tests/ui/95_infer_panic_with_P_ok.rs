// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// Does something.
fn rvs_bail(msg: &str) {
    let _ = msg;
}

#[test]
fn test_20260617_bail_ok() {
    rvs_bail("test");
}
