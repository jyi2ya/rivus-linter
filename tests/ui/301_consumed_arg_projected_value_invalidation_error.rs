#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Holder {
    result: Result<(), u8>,
}

#[inline(never)]
fn rvs_identity(holder: Holder) -> Holder {
    holder
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut holder = rvs_identity(Holder { result: Ok(()) });
    holder.result = Ok(());
    holder = rvs_identity(Holder { result: Err(1) });
    holder.result
}
