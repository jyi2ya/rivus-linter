#![allow(non_snake_case)]

#[derive(Debug)]
struct Holder<'a>(
    &'a String,
    &'a Vec<u8>,
    &'a Box<u8>,
);
