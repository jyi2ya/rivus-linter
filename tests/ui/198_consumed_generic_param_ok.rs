// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Debug)]
enum WriteError {
    Failed,
}

fn rvs_write<W: Copy>(writer: W) -> Result<(), WriteError> {
    let _ = writer;
    if std::hint::black_box(false) {
        Err(WriteError::Failed)
    } else {
        Ok(())
    }
}

#[test]
fn test_20260714_consumed_generic_param_ok() {
    assert!(rvs_write(1_u32).is_ok());
}
