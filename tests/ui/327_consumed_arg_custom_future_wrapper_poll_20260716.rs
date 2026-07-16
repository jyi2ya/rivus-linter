#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

struct Wrapper<F>(F);

impl<F> Future for Wrapper<F> {
    type Output = Result<(), u8>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Err(1))
    }
}

async fn rvs_marker_A() -> Result<(), u8> {
    Ok(())
}

fn rvs_process_M(data: String, context: &mut Context<'_>) -> Result<(), u8> {
    drop(data);
    let mut future = Wrapper(rvs_marker_A());
    let mut pinned = unsafe { Pin::new_unchecked(&mut future) };
    match Future::poll(pinned.as_mut(), context) {
        Poll::Ready(result) => result,
        Poll::Pending => Ok(()),
    }
}
