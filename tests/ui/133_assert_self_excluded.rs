// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct Foo;

impl Foo {
    fn rvs_compute(&self, x: i32) -> i32 {
        x * 2
    }
}

#[test]
fn test_20260612_assert_self_excluded() {
    let foo = Foo;
    foo.rvs_compute(5);
}
