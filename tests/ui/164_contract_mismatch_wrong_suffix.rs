#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_ok_fn)]

trait FetchApi {
    type World;

    fn rvs_fetch_BI(_world: &Self::World) -> i32 {
        1
    }
}

#[derive(Debug)]
struct Api;

#[derive(Debug)]
struct ApiWorld;

impl FetchApi for Api {
    type World = ApiWorld;
}

#[test]
fn test_20260703_fetch_default() {
    let _ = Api::rvs_fetch_BI(&ApiWorld);
}
