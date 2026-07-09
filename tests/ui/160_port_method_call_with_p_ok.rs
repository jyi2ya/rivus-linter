// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_empty_fn)]

trait UserRepository {
    fn rvs_find_by_id_P(&self, id: u64);
}

#[derive(Debug)]
struct MemoryRepo;

impl UserRepository for MemoryRepo {
    fn rvs_find_by_id_P(&self, id: u64) {
        debug_assert!(id > 0);
    }
}

#[derive(Debug)]
struct Service<R> {
    repo: R,
}

impl<R: UserRepository> Service<R> {
    fn rvs_load_P(&self) {
        self.repo.rvs_find_by_id_P(1);
    }
}

#[test]
fn test_20260702_port_method_call_with_p_ok() {
    let service = Service { repo: MemoryRepo };
    service.rvs_load_P();
}
