// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_env_BS() {
    let _ = std::env::var("HOME");
}

fn rvs_caller_BS() {
    rvs_env_BS();
}

#[test]
fn test_20260612_call_compliant_ok() {
    rvs_caller_BS();
}
