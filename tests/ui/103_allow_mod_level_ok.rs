// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[allow(non_snake_case)]
mod inner {
    /// Deep function.
    pub fn rvs_deep_BI() {
        let _ = 42;
    }
}

#[test]
fn test_20260612_allow_mod_level_ok() {
    inner::rvs_deep_BI();
}
