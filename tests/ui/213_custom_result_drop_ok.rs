// check-pass
#![allow(non_snake_case)]

static DROPS: u8 = 0;

struct Result;

fn rvs_drop_custom_result_S() -> u8 {
    drop(Result);
    DROPS
}
