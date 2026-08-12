#![feature(register_tool)]
#![register_tool(rivus)]

trait FetchApi {
    type World;

    fn fetch(_world: &Self::World) -> i32 {
        1
    }
}
