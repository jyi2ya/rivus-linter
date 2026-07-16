#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[derive(Clone, Copy)]
struct Holder {
    result: Result<(), u8>,
}

fn rvs_process(data: String, mut holder: Holder, replace: bool) -> Result<(), u8> {
    drop(data);
    if !replace {
        holder.result = Ok(());
    }
    holder.result
}
