#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[derive(Clone, Copy)]
#[repr(u8)]
enum Choice {
    Safe = 5,
    Error = 9,
}

fn rvs_process(data: String, choice: Choice) -> Result<(), u8> {
    drop(data);
    match choice {
        Choice::Safe => Ok(()),
        Choice::Error => Err(1),
    }
}
