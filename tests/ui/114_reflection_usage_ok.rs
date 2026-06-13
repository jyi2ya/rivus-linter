// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_process(value: &dyn std::fmt::Debug) -> String {
    format!("{value:?}")
}

#[test]
fn test_20260612_reflection_usage_ok() {
    rvs_process(&42);
}
