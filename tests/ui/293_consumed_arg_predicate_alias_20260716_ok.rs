// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_maybe(fail: bool) -> Result<(), u8> {
    if fail { Err(1) } else { Ok(()) }
}

fn rvs_process(data: String, fail: bool) -> Result<(), u8> {
    drop(data);
    let result = rvs_maybe(fail);
    let is_error = result.is_err();
    if is_error {
        return Ok(());
    }
    result
}
