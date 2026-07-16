#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

macro_rules! break_error {
    () => {
        break Err(std::io::Error::other("failed"));
    };
}

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    loop {
        break_error!();
    }
}
