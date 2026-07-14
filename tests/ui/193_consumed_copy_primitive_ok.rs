// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
enum RunError {
    Disabled,
}

fn rvs_run(enabled: bool) -> Result<(), RunError> {
    if enabled {
        Ok(())
    } else {
        Err(RunError::Disabled)
    }
}

#[test]
fn test_20260714_consumed_copy_primitive_ok() {
    assert!(rvs_run(true).is_ok());
}
