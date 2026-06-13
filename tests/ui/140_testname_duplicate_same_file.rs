// compile-flags: --test
#![allow(non_snake_case)]

#[test]
fn test_20260612_dup_case() {}

mod inner {
    #[test]
    fn test_20260612_dup_case() {}
}
