// compile-flags: --test
#![allow(non_snake_case)]

#[test]
fn test_20260612_triplicate() {}

mod a {
    #[test]
    fn test_20260612_triplicate() {}
}

mod b {
    #[test]
    fn test_20260612_triplicate() {}
}
