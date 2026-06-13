// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_handle_ABI() {
    let _ = 42;
}

fn rvs_outer(x: Option<i32>) {
    let Some(_v) = x else {
        rvs_handle_ABI();
        return;
    };
}

#[test]
fn test_20260612_calls_in_let_else() {
    rvs_outer(Some(5));
}
