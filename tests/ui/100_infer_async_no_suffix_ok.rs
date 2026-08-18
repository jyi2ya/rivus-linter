// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn rvs_fetch() {
    let _ = 42;
}

#[test]
fn test_20260816_infer_async_no_suffix_ok() {
    let mut future = pin!(rvs_fetch());
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_ready());
}
