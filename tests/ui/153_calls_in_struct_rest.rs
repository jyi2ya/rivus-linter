// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct Foo {
    x: i32,
    y: i32,
}

fn rvs_a_BI() -> i32 {
    1
}

fn rvs_b_BI() -> Foo {
    Foo { x: 0, y: 0 }
}

fn rvs_outer() {
    let _ = Foo { x: rvs_a_BI(), ..rvs_b_BI() };
}

#[test]
fn test_20260612_calls_in_struct_rest() {
    rvs_outer();
}
