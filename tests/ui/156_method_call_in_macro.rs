// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct S;

impl S {
    fn rvs_compute(&self) -> i32 {
        42
    }
}

#[test]
fn test_20260612_method_call_in_macro() {
    let s = S;
    println!("got {}", s.rvs_compute());
}
