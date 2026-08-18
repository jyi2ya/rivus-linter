// compile-flags: --test
#![allow(non_snake_case)]
#![feature(try_blocks)]

fn rvs_inner_BI() {
    let _ = 42;
}

fn rvs_outer() {
    let _: Result<(), ()> = try {
        rvs_inner_BI();
    };
}

#[test]
fn test_20260612_calls_in_try_block() {
    rvs_outer();
}
