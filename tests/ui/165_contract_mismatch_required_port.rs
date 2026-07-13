#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

trait ApiClient {
    fn rvs_fetch_BI(&self) -> i32;
}
