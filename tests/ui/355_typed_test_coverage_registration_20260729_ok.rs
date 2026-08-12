// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![feature(rustc_attrs)]
#![register_tool(rivus)]
#![allow(internal_features)]
#![allow(non_snake_case)]

#[rustc_diagnostic_item = "rivus_test_coverage_registration"]
fn rvs_register_test_coverage<T: Copy>(_target: T) {
    let _ = std::mem::size_of::<T>();
}

fn rvs_target() -> usize {
    7
}

#[test]
fn test_20260729_typed_test_coverage_registration() {
    rvs_register_test_coverage(rvs_target);
}
