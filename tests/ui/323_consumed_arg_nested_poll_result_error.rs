#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use std::task::Poll;

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let poll: Poll<Result<Result<(), u8>, u8>> = Poll::Ready(Ok(Err(1)));
    match poll {
        Poll::Ready(Ok(inner)) => inner,
        Poll::Ready(Err(_)) | Poll::Pending => Ok(()),
    }
}
