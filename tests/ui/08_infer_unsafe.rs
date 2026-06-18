#![allow(non_snake_case)]

fn rvs_add() {}

pub unsafe fn rvs_raw_ptr(x: *const u8) -> u8 {
    rvs_add();
    *x
}
