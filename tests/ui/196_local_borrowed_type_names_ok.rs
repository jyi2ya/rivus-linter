// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

struct String;

fn rvs_read(value: &String) -> usize {
    std::mem::size_of_val(value)
}

#[test]
fn test_20260714_local_borrowed_type_names_ok() {
    assert_eq!(rvs_read(&String), 0);
}
