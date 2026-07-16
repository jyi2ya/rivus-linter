// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_loop(data: String) -> Result<(), std::io::Error> {
    drop(data);
    loop {
        break Ok(());
    }
}

fn rvs_block(data: String) -> Result<(), std::io::Error> {
    drop(data);
    'result: {
        break 'result Ok(());
    }
}
