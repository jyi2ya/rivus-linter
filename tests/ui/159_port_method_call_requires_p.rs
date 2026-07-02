// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_ok_fn)]

trait UserRepository {
    fn rvs_find_by_id_ABI(&self, id: u64);
}

#[derive(Debug)]
struct MemoryRepo;

impl UserRepository for MemoryRepo {
    fn rvs_find_by_id_ABI(&self, id: u64) {
        debug_assert!(id > 0);
    }
}

#[derive(Debug)]
struct Service<R> {
    repo: R,
}

impl<R: UserRepository> Service<R> {
    fn rvs_load(&self) {
        self.repo.rvs_find_by_id_ABI(1);
    }
}

#[test]
fn test_20260702_port_method_call_requires_p() {
    let service = Service { repo: MemoryRepo };
    service.rvs_load();
}
