// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_default_option() -> usize {
    None::<usize>.unwrap_or_default()
}

#[test]
fn test_20260714_error_swallow_option_default_ok() {
    assert_eq!(rvs_default_option(), 0);
}
