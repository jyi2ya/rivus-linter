// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    let local_error: Result<(), std::io::Error> = loop {
        break Err(std::io::Error::other("handled locally"));
    };
    match local_error {
        Ok(()) | Err(_) => {}
    }
    Ok(())
}
