// check-pass
// compile-flags: --test
// Tests that calls inside guard pattern conditions are collected by the
// HIR visitor. The guard pattern calls rvs_check_BIS which propagates
// BIS. If the visitor fails to walk patterns, the caller would lose its
// caps and the naming contract would fail.
#![feature(register_tool)]
#![register_tool(rivus)]
#![feature(guard_patterns)]
#![allow(non_snake_case)]
#![allow(incomplete_features)]

fn rvs_check_BIS(x: i32) -> bool {
    debug_assert!(x >= i32::MIN);
    let _ = std::fs::remove_file("fixture-marker");
    x > 0
}

fn rvs_guard_pattern_caller_BIS(x: Option<i32>) {
    match x {
        Some(x if rvs_check_BIS(x)) => {}
        _ => {}
    }
}

#[test]
fn test_20260810_guard_pattern_call_collected() {
    rvs_guard_pattern_caller_BIS(Some(5));
}
