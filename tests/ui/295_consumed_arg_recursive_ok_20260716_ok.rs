// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

enum Depth {
    Zero,
    More(Box<Depth>),
}

fn rvs_recursive_ok(depth: Depth) -> Result<(), u8> {
    match depth {
        Depth::Zero => Ok(()),
        Depth::More(next) => rvs_recursive_ok(*next),
    }
}

fn rvs_process(data: String, depth: Depth) -> Result<(), u8> {
    drop(data);
    rvs_recursive_ok(depth)
}
