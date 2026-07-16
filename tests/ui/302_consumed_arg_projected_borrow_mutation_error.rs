#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Holder {
    result: Result<(), u8>,
}

#[inline(never)]
fn rvs_set_error_M(result: &mut Result<(), u8>) {
    *result = Err(1);
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut holder = Holder { result: Ok(()) };
    holder.result = Ok(());
    rvs_set_error_M(&mut holder.result);
    holder.result
}
