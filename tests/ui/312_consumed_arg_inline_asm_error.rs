#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[cfg(target_arch = "x86_64")]
#[inline(never)]
unsafe extern "C" fn rvs_set_error_U(pointer: *mut Result<(), u8>) {
    unsafe {
        *pointer = Err(8);
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn rvs_process_U(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let pointer = &raw mut result;
    unsafe {
        core::arch::asm!(
            "call {setter}",
            setter = sym rvs_set_error_U,
            in("rdi") pointer,
            clobber_abi("C"),
        );
    }
    result
}
