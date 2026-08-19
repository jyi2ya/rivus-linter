// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct Foo {
    x: i32,
    y: i32,
}

fn rvs_a_BIS() -> i32 {
    let _ = std::fs::remove_file("fixture-marker");
    1
}

fn rvs_b_BIS() -> Foo {
    let _ = std::fs::remove_file("fixture-marker");
    Foo { x: 0, y: 0 }
}

fn rvs_outer_BIS() {
    let _ = Foo { x: rvs_a_BIS(), ..rvs_b_BIS() };
}

#[test]
fn test_20260612_calls_in_struct_rest() {
    rvs_outer_BIS();
}
