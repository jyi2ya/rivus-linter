// check-pass
#![allow(non_snake_case)]

trait FetchUsers {
    type World;

    async fn rvs_fetch_P(world: &Self::World, id: u64);
}
