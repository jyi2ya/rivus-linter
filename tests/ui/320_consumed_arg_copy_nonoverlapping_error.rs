#![feature(core_intrinsics, register_tool)]
#![register_tool(rivus)]
#![allow(internal_features)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_safety_doc)]
#![allow(rivus::rvs_untested_good_fn)]

unsafe fn rvs_process_U(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let error = Err(1_u8);
    unsafe {
        core::intrinsics::copy_nonoverlapping(&raw const error, &raw mut result, 1);
    }
    result
}
