// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_good_ABIS() {
    let _ = 42;
}

#[test]
fn test_20260612_spawn_no_spawn_ok() {
    rvs_good_ABIS();
}
