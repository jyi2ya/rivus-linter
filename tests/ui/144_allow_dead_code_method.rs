// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct Foo;

impl Foo {
    #[allow(dead_code)]
    fn rvs_orphan(&self) {
        let _ = 42;
    }

    fn rvs_used(&self) {
        let _ = 42;
    }
}

#[test]
fn test_20260612_allow_dead_code_method() {
    Foo.rvs_used();
}
