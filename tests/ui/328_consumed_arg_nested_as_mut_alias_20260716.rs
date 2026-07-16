#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut inner = Ok(());
    let mut outer: Result<&mut Result<(), u8>, u8> = Ok(&mut inner);
    if let Ok(slot) = outer.as_mut() {
        **slot = Err(1);
    }
    inner
}
