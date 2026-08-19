// check-pass
// compile-flags: --test
// The block yields the function item itself; calling it is a weak-edge
// reference that still propagates BIS to the caller.
#![allow(non_snake_case)]

fn rvs_blocking_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_wrapped_BIS() {
    ({ rvs_blocking_BIS })();
}

#[test]
fn test_20260714_block_wrapped_callee() {
    rvs_wrapped_BIS();
}
