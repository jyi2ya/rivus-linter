#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

#[derive(Clone, Copy)]
struct Pair {
    first: Result<(), u8>,
    second: Result<(), u8>,
}

fn rvs_process(data: String, pair: Pair) -> Result<(), u8> {
    drop(data);
    match pair.first {
        Ok(()) => match pair.second {
            Ok(()) => Ok(()),
            Err(error) => Err(error),
        },
        Err(_) => Ok(()),
    }
}
