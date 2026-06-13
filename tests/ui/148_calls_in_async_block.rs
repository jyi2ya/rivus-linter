// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_inner_ABI() {
    let _ = 42;
}

fn rvs_outer() {
    let _ = async {
        rvs_inner_ABI();
    };
}

#[test]
fn test_20260612_calls_in_async_block() {
    rvs_outer();
}
