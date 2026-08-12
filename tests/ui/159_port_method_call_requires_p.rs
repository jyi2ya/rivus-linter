// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_empty_fn)]

trait LookupUsers {
    type World;

    fn rvs_find_by_id_P(world: &Self::World, id: u64);
}

#[derive(Debug)]
struct MemoryWorld;

#[derive(Debug)]
struct MemoryEffects;

impl LookupUsers for MemoryEffects {
    type World = MemoryWorld;

    fn rvs_find_by_id_P(_world: &Self::World, id: u64) {
        debug_assert!(id > 0);
    }
}

fn rvs_load<E: LookupUsers>(world: &E::World) {
    E::rvs_find_by_id_P(world, 1);
}

#[test]
fn test_20260702_port_method_call_requires_p() {
    rvs_load::<MemoryEffects>(&MemoryWorld);
}
