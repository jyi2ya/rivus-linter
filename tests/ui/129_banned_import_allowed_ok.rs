// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_banned_imports_ok() {
    use std::collections::HashMap;
    let _: Option<HashMap<i32, i32>> = None;
}

#[test]
fn test_20260612_banned_import_allowed_ok() {
    rvs_banned_imports_ok();
}
