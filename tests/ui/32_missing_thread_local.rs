#![expect(non_snake_case)]

use std::cell::RefCell;

thread_local! {
    static COUNTER: RefCell<u32> = RefCell::new(0);
}

fn rvs_get_tls() -> u32 {
    COUNTER.with(|c| *c.borrow())
}
