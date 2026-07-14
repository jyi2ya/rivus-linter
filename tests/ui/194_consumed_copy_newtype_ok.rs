// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

#[derive(Clone, Copy, Debug)]
struct Token(u8);

#[derive(Debug)]
enum RunError {
    Failed,
}

fn rvs_run(token: Token) -> Result<(), RunError> {
    if token.0 == 0 {
        Err(RunError::Failed)
    } else {
        Ok(())
    }
}

#[test]
fn test_20260714_consumed_copy_newtype_ok() {
    assert!(rvs_run(Token(1)).is_ok());
}
