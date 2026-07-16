#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use std::task::Poll;

async fn rvs_marker_A() -> Result<(), u8> {
    Ok(())
}

#[inline(never)]
fn rvs_fake_poll<F>(_future: &F) -> Poll<Result<(), u8>> {
    Poll::Ready(Err(1))
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let future = rvs_marker_A();
    match rvs_fake_poll(&future) {
        Poll::Ready(result) => result,
        Poll::Pending => Ok(()),
    }
}
