#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_ok_fn)]

trait ApiClient {
    fn rvs_fetch_BI(&self) -> i32 {
        1
    }
}

#[derive(Debug)]
struct Api;

impl ApiClient for Api {}

#[test]
fn test_20260703_fetch_default() {
    let _ = Api.rvs_fetch_BI();
}

fn main() {}
