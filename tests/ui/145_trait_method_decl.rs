// check-pass
#![allow(non_snake_case)]

trait StoreUsers {
    type World;

    fn rvs_find_by_id_P(world: &Self::World, id: u64);
    fn rvs_save_MP(world: &mut Self::World, data: &str);
}
