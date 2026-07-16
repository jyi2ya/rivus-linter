#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_invoke_M<F: FnMut()>(mut callback: F) {
    callback();
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let set_error = || result = Err(1);
    rvs_invoke_M(set_error);
    result
}
