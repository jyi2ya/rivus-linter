#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

unsafe fn rvs_process_U(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let pointer = &raw mut result;
    let pointers = std::hint::black_box([pointer; 1]);
    let copied = pointers[0];
    unsafe {
        *copied = Err(6);
    }
    result
}
