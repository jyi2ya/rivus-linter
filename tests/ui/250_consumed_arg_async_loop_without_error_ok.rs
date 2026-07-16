// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

async fn rvs_wait_A() -> Result<(), std::io::Error> {
    std::hint::black_box(Ok(()))
}

async fn rvs_process_A(data: String) -> Result<(), std::io::Error> {
    drop(data);
    loop {
        if let Err(error) = rvs_wait_A().await {
            drop(error);
            return Ok(());
        }
        return Ok(());
    }
}
