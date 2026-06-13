// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_write_ABI() {
    let _ = 42;
}

fn rvs_caller_ABI() {
    rvs_write_ABI();
}

#[test]
fn test_20260612_call_compliant_ok() {
    rvs_caller_ABI();
}
