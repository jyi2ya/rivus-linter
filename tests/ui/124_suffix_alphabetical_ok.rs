// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_foo_AB() {
    let _ = 42;
}

#[test]
fn test_20260612_suffix_alphabetical_ok() {
    rvs_foo_AB();
}
