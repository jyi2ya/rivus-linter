#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(value: (String, u8)) -> Result<(), ()> {
    drop(value);
    Err(())
}
