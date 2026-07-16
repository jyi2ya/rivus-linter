#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_missing_safety_doc)]
#![allow(rivus::rvs_untested_good_fn)]

union Pair {
    first: Result<(), u8>,
    second: Result<(), u8>,
}

unsafe fn rvs_process_U(data: String) -> Result<(), u8> {
    drop(data);
    let mut pair = Pair { first: Ok(()) };
    pair.first = Ok(());
    let _ = std::hint::black_box(unsafe { pair.first });
    pair.second = Err(1);
    unsafe { pair.first }
}
