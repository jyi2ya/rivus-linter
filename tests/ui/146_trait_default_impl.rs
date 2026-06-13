// compile-flags: --test
#![allow(non_snake_case)]

trait Handler {
    fn rvs_handle(&self, x: i32) {
        let _ = x + 1;
    }
}

#[derive(Debug)]
struct MyHandler;

impl Handler for MyHandler {}

#[test]
fn test_20260612_trait_default_impl() {
    let h = MyHandler;
    h.rvs_handle(5);
}
