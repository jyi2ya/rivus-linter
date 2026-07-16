#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let was_ok = result.is_ok();
    result = Err(1);
    if was_ok { result } else { Ok(()) }
}
