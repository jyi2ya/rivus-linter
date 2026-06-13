// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_add(a: i32, b: i32) -> i32 {
    debug_assert!(a >= 0);
    debug_assert!(b >= 0);
    a + b
}

#[test]
fn test_20260612_format_macro_call() {
    let _ = format!("result: {}", rvs_add(1, 2));
}
