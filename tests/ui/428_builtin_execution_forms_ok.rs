// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![deny(rivus::rvs_unsupported_implicit_execution)]

fn rvs_builtin_operators() -> i32 {
    let mut values = [1, 2];
    values[0] += 1;
    let dynamic = vec![String::from("same")];
    let same = dynamic[0] == "same";
    i32::from(same) - values[0] + values[1]
}

#[test]
fn test_20260811_builtin_execution_forms_remain_supported() {
    assert_eq!(rvs_builtin_operators(), 1);
}
