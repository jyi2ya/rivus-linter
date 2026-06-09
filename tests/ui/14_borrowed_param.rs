#![expect(non_snake_case)]

pub struct Foo<'a> {
    name: &'a String,
    data: &'a Vec<u8>,
    ptr: &'a Box<i32>,
}

pub fn rvs_example(x: &String, y: &Vec<u8>) {}
