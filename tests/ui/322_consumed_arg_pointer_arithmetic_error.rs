#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_safety_doc)]
#![allow(rivus::rvs_untested_good_fn)]

unsafe fn rvs_process_U(data: String, offset: usize) -> Result<(), u8> {
    debug_assert_eq!(offset, 0);
    drop(data);
    let mut result = Ok(());
    let pointer = &raw mut result;
    let address = pointer as usize;
    let adjusted = address + offset;
    let reconstructed = adjusted as *mut Result<(), u8>;
    unsafe {
        *reconstructed = Err(1);
    }
    result
}
