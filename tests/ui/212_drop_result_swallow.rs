#![allow(non_snake_case)]

#[derive(Debug)]
struct Error;

fn rvs_drop_result_swallow_S() {
    let result: Result<(), Error> = Err(Error);
    drop(result);
}
