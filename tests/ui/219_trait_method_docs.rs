#![allow(non_snake_case)]

pub trait Service {
    unsafe fn rvs_required();

    unsafe fn rvs_provided() {
        let _ = 42;
    }
}
