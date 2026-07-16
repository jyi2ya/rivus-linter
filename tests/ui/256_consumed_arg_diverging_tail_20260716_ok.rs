// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_diverge() -> ! {
    loop {}
}

fn rvs_process_with_panic(data: String) -> Result<(), std::io::Error> {
    drop(data);
    panic!("never returns")
}

fn rvs_process_with_helper(data: String) -> Result<(), std::io::Error> {
    drop(data);
    rvs_diverge()
}
