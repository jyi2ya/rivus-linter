// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_borrowed_result_swallow_S() {
    let result: Result<u8, u8> = Ok(1);
    let borrowed = &result;
    let _ = borrowed.ok();
}

#[test]
fn test_20260714_borrowed_result_swallow() {
    rvs_borrowed_result_swallow_S();
}
