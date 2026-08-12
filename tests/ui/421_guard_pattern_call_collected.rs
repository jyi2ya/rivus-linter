// compile-flags: --test
// Tests that calls inside guard pattern conditions are collected by the
// HIR visitor. The guard pattern calls rvs_check_ABI which propagates
// BI. If the visitor fails to walk patterns, the caller would compile
// clean — this test catches that regression by requiring the caller to
// declare the propagated capabilities.
#![feature(register_tool)]
#![register_tool(rivus)]
#![feature(guard_patterns)]
#![allow(non_snake_case)]
#![allow(incomplete_features)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_check_ABI(x: i32) -> bool {
    debug_assert!(x >= i32::MIN);
    x > 0
}

fn rvs_guard_pattern_caller(x: Option<i32>) {
    match x {
        Some(x if rvs_check_ABI(x)) => {}
        _ => {}
    }
}

#[test]
fn test_20260810_guard_pattern_call_collected() {
    rvs_guard_pattern_caller(Some(5));
}
