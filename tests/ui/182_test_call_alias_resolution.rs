// compile-flags: --test
#![allow(non_snake_case)]

mod actual {
    pub(crate) fn rvs_real() -> i32 {
        1
    }
}

mod decoy {
    pub(crate) fn rvs_decoy() -> i32 {
        2
    }
}

mod victim {
    pub(crate) fn rvs_victim() -> i32 {
        3
    }
}

#[test]
fn test_20260710_test_call_alias_resolution() {
    use actual::rvs_real as renamed;
    use decoy::rvs_decoy as rvs_victim;

    let _ = renamed();
    let _ = rvs_victim();
}
