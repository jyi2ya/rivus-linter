// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_write_ABI() {
    let _ = 42;
}

#[test]
fn test_20260612_allow_present_on_fn_ok() {
    rvs_write_ABI();
}
