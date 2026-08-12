#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_allow)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

fn rvs_read_BI() -> i32 {
    1
}

trait FetchApi {
    type World;

    fn rvs_fetch_P(_world: &Self::World) -> i32 {
        rvs_read_BI()
    }
}
