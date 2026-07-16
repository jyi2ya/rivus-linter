// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

async fn rvs_process_A(data: String) -> Result<(), u8> {
    drop(data);
    let result: Result<(), u8> = Ok(());
    std::future::ready(()).await;
    result
}
