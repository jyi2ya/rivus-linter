#![allow(non_snake_case)]

struct Guard;

impl Guard {
    fn catch_unwind(&self) {}
}

fn rvs_run() {
    Guard.catch_unwind();
}
