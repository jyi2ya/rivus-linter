// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

async fn rvs_fetch_A() {
    let _ = 42;
}

#[test]
fn test_20260612_infer_async_with_A_ok() {
    let _ = rvs_fetch_A();
}
