// check-pass
#![feature(register_tool, try_blocks)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    let _: Result<(), std::io::Error> = try {
        Err(std::io::Error::other("caught"))?;
    };
    Ok(())
}
