#![expect(non_snake_case)]

pub fn rvs_swallow() {
    let x: Result<i32, ()> = Ok(5);
    x.ok();
    let y: Result<i32, ()> = Ok(5);
    y.unwrap_or_default();
}
