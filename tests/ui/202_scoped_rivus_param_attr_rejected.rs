// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_borrowed(#[expect(rivus::rvs_borrowed_param)] value: &String) -> usize {
    value.len()
}

fn rvs_consumed(
    #[expect(rivus::rvs_consumed_arg_on_error)] value: String,
) -> Result<(), ()> {
    drop(value);
    Err(())
}

#[test]
fn test_20260714_parameter_expectations_ok() {
    assert_eq!(rvs_borrowed(&String::new()), 0);
    assert_eq!(rvs_consumed(String::new()), Err(()));
}
