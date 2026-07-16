#![feature(try_blocks)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_fallible(fail: bool) -> Result<(), std::io::Error> {
    if fail {
        Err(std::io::Error::other("failed"))
    } else {
        Ok(())
    }
}

fn rvs_process(data: String, fail: bool) -> Result<(), std::io::Error> {
    drop(data);
    try {
        Ok::<(), std::io::Error>({
            rvs_fallible(fail)?;
        })?;
    }
}
