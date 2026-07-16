// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Holder {
    result: Result<(), u8>,
}

#[inline(never)]
fn rvs_maybe(fail: bool) -> Result<(), u8> {
    if fail { Err(1) } else { Ok(()) }
}

fn rvs_process(data: String, fail: bool) -> Result<(), u8> {
    drop(data);
    let holder = std::hint::black_box(Holder {
        result: rvs_maybe(fail),
    });
    if holder.result.is_err() {
        return Ok(());
    }
    holder.result
}
