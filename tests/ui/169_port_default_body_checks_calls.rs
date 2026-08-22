// check-pass
// The default body calls a BIS helper. Under the fixed-P contract the Port
// implementation is not checked against the public contract: implementation
// effects are audit information, so this compiles clean. The default body's
// call collection is asserted positively by
// test_20260822_port_default_body_calls_reach_trait_votes in src/inference.rs.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_allow)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

fn rvs_read_BIS() -> i32 {
    let _ = std::fs::remove_file("fixture-marker");
    1
}

trait FetchApi {
    type World;

    fn rvs_fetch_P(_world: &Self::World) -> i32 {
        rvs_read_BIS()
    }
}

struct Api;
struct ApiWorld;

impl FetchApi for Api {
    type World = ApiWorld;

    fn rvs_fetch_P(_world: &ApiWorld) -> i32 {
        0
    }
}
