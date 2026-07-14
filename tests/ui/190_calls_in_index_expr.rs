// compile-flags: --test
#![allow(non_snake_case)]

static INDEX: usize = 0;

fn rvs_index_S() -> usize {
    INDEX
}

fn rvs_read(values: &[usize]) -> usize {
    values[rvs_index_S()]
}

#[test]
fn test_20260714_calls_in_index_expr() {
    assert_eq!(rvs_read(&[7]), 7);
}
