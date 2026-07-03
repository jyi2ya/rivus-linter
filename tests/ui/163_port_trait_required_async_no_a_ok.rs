// check-pass
#![allow(non_snake_case)]

trait UserRepository {
    async fn rvs_fetch_P(&self, id: u64);
}
