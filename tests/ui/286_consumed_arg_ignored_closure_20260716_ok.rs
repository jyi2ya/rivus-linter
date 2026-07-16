// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[inline(never)]
fn rvs_ignore<F>(callback: F) {
    let _callback = std::hint::black_box(callback);
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let set_error = || result = Err(1);
    rvs_ignore(set_error);
    result
}
