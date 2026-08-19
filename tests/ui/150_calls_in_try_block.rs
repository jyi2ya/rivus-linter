// check-pass
// compile-flags: --test
#![allow(non_snake_case)]
#![feature(try_blocks)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_outer_BIS() {
    let _: Result<(), ()> = try {
        rvs_inner_BIS();
    };
}

#[test]
fn test_20260612_calls_in_try_block() {
    rvs_outer_BIS();
}
