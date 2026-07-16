// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    Ok::<(), std::io::Error>(())?;
    Ok(())
}
