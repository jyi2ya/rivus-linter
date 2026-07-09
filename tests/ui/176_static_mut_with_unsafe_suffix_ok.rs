// check-pass
#![allow(non_snake_case)]

static mut COUNTER: u32 = 0;

fn rvs_read_counter_SU() -> u32 {
    unsafe { COUNTER }
}
