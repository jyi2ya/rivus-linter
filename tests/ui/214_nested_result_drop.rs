#![allow(non_snake_case)]

#[derive(Debug)]
struct Error;

type Fallible = Result<(), Error>;

fn rvs_nested_result_drop_S() {
    let _closure = || {
        let result: Fallible = Err(Error);
        drop(result);
    };
    _closure();
}
