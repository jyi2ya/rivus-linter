// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_infallible() -> Result<(), std::convert::Infallible> {
    Ok(())
}

fn rvs_process(data: String) -> Result<(), std::convert::Infallible> {
    drop(data);
    rvs_infallible()?;
    Ok(())
}
