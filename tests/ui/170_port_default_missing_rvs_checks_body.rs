#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_contract_mismatch)]
#![allow(rivus::rvs_missing_allow)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

fn rvs_read_BI() -> i32 {
    1
}

trait ApiClient {
    fn fetch(&self) -> i32 {
        rvs_read_BI()
    }
}
