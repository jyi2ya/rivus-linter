#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String, fail: bool) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    if fail {
        result.clone_from(&Err(1));
    }
    result
}
