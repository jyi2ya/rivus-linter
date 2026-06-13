#![allow(non_snake_case)]

struct Foo;
impl Foo {
    fn rvs_modify(&mut self) {
        let _ = 42;
    }
}
