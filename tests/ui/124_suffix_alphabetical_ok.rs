// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

static SEED: u8 = 7;

fn rvs_foo_S() -> u8 {
    SEED
}

#[test]
fn test_20260612_suffix_alphabetical_ok() {
    let _ = rvs_foo_S();
}
