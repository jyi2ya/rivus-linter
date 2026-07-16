#![allow(non_snake_case)]

#[derive(Debug)]
struct Payload;

fn rvs_validate(payload: Payload) -> std::result::Result<(), std::io::Error> {
    drop(payload);
    Err(std::io::Error::other("failed"))
}
