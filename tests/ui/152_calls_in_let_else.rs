// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_handle_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_outer_BIS(x: Option<i32>) {
    let Some(_v) = x else {
        rvs_handle_BIS();
        return;
    };
}

#[test]
fn test_20260612_calls_in_let_else() {
    rvs_outer_BIS(Some(5));
}
