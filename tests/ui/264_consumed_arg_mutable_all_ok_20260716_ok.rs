// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String, replace: bool) -> Result<(), std::io::Error> {
    drop(data);
    let mut result = Ok(());
    if replace {
        result = Ok(());
    }
    result
}
