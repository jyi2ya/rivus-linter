#![allow(non_snake_case)]

use std::cell::Cell;

thread_local! {
    static TLS_KEY: Cell<u32> = const { Cell::new(0) };
}

fn rvs_read_local_key_S() -> u32 {
    TLS_KEY.get()
}
