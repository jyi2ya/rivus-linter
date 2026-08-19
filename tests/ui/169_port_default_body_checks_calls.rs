// The default body is checked against the impl-voted contract: the pure
// override votes no effects, so the default body's BIS call is a
// port-effect violation. If default-body call collection regressed, this
// fixture would compile clean and the test would fail.
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
