#![allow(non_snake_case)]

pub fn rvs_add() {}

#[test]
fn test_example() {
    tokio::spawn(async { rvs_add(); });
}
