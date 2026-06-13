// check-pass
#![allow(non_snake_case)]

trait Repository {
    fn rvs_find_by_id_ABI(&self, id: u64);
    fn rvs_save_ABI(&self, data: &str);
}
