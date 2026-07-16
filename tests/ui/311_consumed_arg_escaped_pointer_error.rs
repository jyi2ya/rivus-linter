#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use std::sync::atomic::{AtomicPtr, Ordering};

static ESCAPED: AtomicPtr<Result<(), u8>> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn rvs_set_error_SU() {
    let pointer = ESCAPED.load(Ordering::Relaxed);
    unsafe {
        *pointer = Err(7);
    }
}

unsafe fn rvs_process_SU(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    ESCAPED.store(&raw mut result, Ordering::Relaxed);
    result = Ok(());
    unsafe {
        rvs_set_error_SU();
    }
    result
}
