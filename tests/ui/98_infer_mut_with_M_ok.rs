// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_update_M(data: &mut i32) {
    debug_assert!(*data > 0);
    *data = 42;
}

#[test]
fn test_20260612_infer_mut_with_M_ok() {
    let mut x = 5;
    rvs_update_M(&mut x);
}
