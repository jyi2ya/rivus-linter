#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), std::io::Error> {
    drop(data);
    'result: {
        break 'result Err(std::io::Error::other("failed"));
    }
}
