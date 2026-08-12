// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn rvs_compute_A(x: i32) -> i32 {
    debug_assert!(x > 0);
    x + 1
}

#[test]
fn test_20260708_async_debug_assert_ok() {
    let mut future = pin!(rvs_compute_A(1));
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_ready());
}
