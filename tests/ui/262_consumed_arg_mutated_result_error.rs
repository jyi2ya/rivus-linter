#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String, fail: bool) -> Result<(), std::io::Error> {
    drop(data);
    let mut result = Ok(());
    if fail {
        result = Err(std::io::Error::other("failed"));
    }
    result
}
