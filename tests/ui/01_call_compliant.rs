#![expect(non_snake_case)]

fn rvs_add(a: i32, b: i32) -> i32 {
    a + b
}

fn rvs_write_BI() {
    rvs_add(1, 2);
}

fn rvs_read_BI() -> i32 {
    rvs_add(3, 4)
}
