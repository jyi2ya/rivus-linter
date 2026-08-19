// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_env_BS() {
    let _ = std::env::var("HOME");
}

#[test]
fn test_20260612_spawn_no_spawn_ok() {
    rvs_env_BS();
}
