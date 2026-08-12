#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

trait FetchApi {
    type World;

    fn rvs_fetch_BI(world: &Self::World) -> i32;
}
