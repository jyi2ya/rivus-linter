#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[inline(never)]
fn rvs_alias_M(result: &mut Result<(), u8>) -> &mut Result<(), u8> {
    result
}

#[inline(never)]
fn rvs_set_error_M(result: &mut Result<(), u8>) {
    let alias = rvs_alias_M(result);
    *alias = Err(1);
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    rvs_set_error_M(&mut result);
    result
}
