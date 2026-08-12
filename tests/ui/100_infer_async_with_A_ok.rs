// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn rvs_fetch_A() {
    let _ = 42;
}

#[test]
fn test_20260612_infer_async_with_A_ok() {
    let mut future = pin!(rvs_fetch_A());
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_ready());
}
