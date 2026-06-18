#![allow(non_snake_case)]

struct Foo;

impl Into<String> for Foo {
    fn into(self) -> String {
        String::new()
    }
}
