// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
struct Svc;

#[allow(non_snake_case)]
impl Svc {
    fn rvs_run_AI(&self) {
        let _ = 42;
    }
}

#[test]
fn test_20260612_allow_on_impl_block_ok() {
    Svc.rvs_run_AI();
}
