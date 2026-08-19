// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_env_BS() {
    let _ = std::env::var("HOME");
}

#[test]
fn test_20260612_allow_present_on_fn_ok() {
    rvs_env_BS();
}
