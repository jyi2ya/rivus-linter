#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_safety_doc)]
#![allow(rivus::rvs_untested_good_fn)]

use std::sync::atomic::{AtomicPtr, Ordering};

static SAVED: AtomicPtr<Result<(), u8>> = AtomicPtr::new(std::ptr::null_mut());

#[inline(never)]
fn rvs_save_S(pointer: *mut Result<(), u8>) {
    SAVED.store(pointer, Ordering::Relaxed);
}

#[inline(never)]
unsafe fn rvs_set_saved_SU() {
    let pointer = SAVED.load(Ordering::Relaxed);
    unsafe {
        *pointer = Err(1);
    }
}

unsafe fn rvs_process_SU(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    rvs_save_S(&raw mut result);
    result = Ok(());
    unsafe {
        rvs_set_saved_SU();
    }
    result
}
