#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String, fail: bool) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let mut set_error = || result = Err(1);
    if fail {
        set_error();
    }
    result
}
