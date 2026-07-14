// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

use std::marker::PhantomData;

#[derive(Debug)]
struct Result<T, E> {
    value: T,
    error: PhantomData<E>,
}

#[derive(Debug)]
struct ValidationError;

fn rvs_validate() -> Result<(), ValidationError> {
    Result {
        value: (),
        error: PhantomData,
    }
}

#[test]
fn test_20260714_local_result_type_ok() {
    let result = rvs_validate();
    assert_eq!(result.value, ());
}
