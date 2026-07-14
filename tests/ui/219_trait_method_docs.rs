#![allow(non_snake_case)]

pub trait Service {
    unsafe fn rvs_required_U();

    unsafe fn rvs_provided_U() {
        let _ = 42;
    }
}
