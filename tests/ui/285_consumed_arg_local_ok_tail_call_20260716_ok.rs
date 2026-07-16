// check-pass
#![feature(explicit_tail_calls)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(incomplete_features)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[inline(never)]
fn rvs_always_ok(data: String) -> Result<(), u8> {
    drop(data);
    Ok(())
}

fn rvs_process(data: String) -> Result<(), u8> {
    become rvs_always_ok(data)
}
