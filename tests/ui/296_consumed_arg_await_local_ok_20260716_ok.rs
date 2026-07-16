// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

async fn rvs_always_ok_A() -> Result<(), u8> {
    Ok(())
}

async fn rvs_process_A(data: String) -> Result<(), u8> {
    drop(data);
    rvs_always_ok_A().await
}
