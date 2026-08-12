#![feature(register_tool)]
#![register_tool(rivus)]

trait ObjectStyleEffects {
    type World;

    fn fetch(&self, world: &mut Self::World);
}
