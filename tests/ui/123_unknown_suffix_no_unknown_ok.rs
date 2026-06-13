// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_good_ABIM() {
    let _ = 42;
}

#[test]
fn test_20260612_unknown_suffix_no_unknown_ok() {
    rvs_good_ABIM();
}
