// compile-flags: --test
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_indirect_call)]

fn rvs_inner_ABI() {
    let _ = 42;
}

fn rvs_outer() {
    let closure = || {
        rvs_inner_ABI();
    };
    closure();
}

#[test]
fn test_20260612_calls_in_closure() {
    rvs_outer();
}
