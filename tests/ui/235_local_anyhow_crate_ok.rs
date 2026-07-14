// check-pass
// compile-flags: --crate-name=anyhow --test
#![allow(non_snake_case)]

struct Error;

mod consumer {
    use crate::Error;

    fn rvs_error_size() -> usize {
        core::mem::size_of::<Error>()
    }

    #[test]
    fn test_20260714_local_anyhow_crate_ok() {
        assert_eq!(rvs_error_size(), 0);
    }
}
