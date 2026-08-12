#![feature(register_tool)]
#![register_tool(rivus)]

trait BorrowingEffects {
    type World<'a>;

    fn fetch(world: &Self::World<'_>);
}
