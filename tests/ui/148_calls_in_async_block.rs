// check-pass
// compile-flags: --test
// The async-block call must propagate BIS to the caller: if collection
// of calls inside async blocks regresses, rvs_outer_BIS loses its caps
// and the naming contract fails.
#![allow(non_snake_case)]

fn rvs_inner_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_outer_BIS() {
    let _ = async {
        rvs_inner_BIS();
    };
}

#[test]
fn test_20260612_calls_in_async_block() {
    rvs_outer_BIS();
}
