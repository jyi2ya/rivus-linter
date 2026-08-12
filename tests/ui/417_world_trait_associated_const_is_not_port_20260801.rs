#![feature(register_tool)]
#![register_tool(rivus)]

trait ConfiguredEffects {
    type World;
    const RETRIES: usize;

    fn fetch(world: &Self::World);
}
