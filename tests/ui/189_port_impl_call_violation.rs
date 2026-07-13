#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_ok_fn)]

fn rvs_effect_S() -> i32 {
    1
}

trait ApiClient {
    fn rvs_fetch_P(&self) -> i32;
}

#[derive(Debug)]
struct Api;

impl ApiClient for Api {
    fn rvs_fetch_P(&self) -> i32 {
        rvs_effect_S()
    }
}
