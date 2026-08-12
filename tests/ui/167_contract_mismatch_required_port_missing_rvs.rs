#![feature(register_tool)]
#![register_tool(rivus)]

trait FetchApi {
    type World;

    fn fetch(world: &Self::World) -> i32;
}
