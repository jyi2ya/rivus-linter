// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[inline(never)]
fn rvs_observe_M(result: &mut Result<(), u8>, value: &str) {
    let _ = std::cmp::max(value, "minimum");
    let _ = result.as_mut();
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    rvs_observe_M(&mut result, "value");
    result
}
