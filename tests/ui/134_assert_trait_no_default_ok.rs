// check-pass
#![allow(non_snake_case)]

trait LookupUsers {
    type World;

    fn rvs_find_by_id_P(world: &Self::World, id: u64);
}
