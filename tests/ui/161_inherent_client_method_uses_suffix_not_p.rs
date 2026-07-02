#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

#[derive(Debug)]
struct HttpClient;

impl HttpClient {
    fn rvs_fetch_BI(&self) {
        panic!("never: compile-only UI fixture");
    }
}

fn rvs_run() {
    let client = HttpClient;
    client.rvs_fetch_BI();
}
