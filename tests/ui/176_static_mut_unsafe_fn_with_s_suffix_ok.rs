// check-pass
#![allow(non_snake_case)]

static mut COUNTER: u32 = 0;

/// Reads the counter.
///
/// # Safety
///
/// Caller must guarantee no concurrent mutation.
unsafe fn rvs_read_counter_S() -> u32 {
    unsafe { COUNTER }
}
