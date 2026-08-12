#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

trait MixedEffects {
    type World;

    fn rvs_read_P(world: &Self::World);
    fn write(bytes: &[u8]);
}
