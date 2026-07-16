#![feature(explicit_tail_calls)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(incomplete_features)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[allow(rivus::rvs_consumed_arg_on_error)]
fn rvs_fail(data: String, _fail: bool) -> Result<(), std::io::Error> {
    drop(data);
    Err(std::io::Error::other("failed"))
}

fn rvs_process(data: String, fail: bool) -> Result<(), std::io::Error> {
    if fail {
        become rvs_fail(data, fail)
    }
    drop(data);
    Ok(())
}
