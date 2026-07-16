// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String, choose: bool) -> Result<(), u8> {
    drop(data);
    let mut local: Result<u8, u8> = if choose { Ok(1) } else { Ok(2) };
    match local.as_mut() {
        Ok(_) => Ok(()),
        Err(_) => Err(1),
    }
}
