// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_untested_ok_fn)]

#[derive(Debug)]
enum Outcome {
    Ready(i32),
}

fn rvs_make() -> Outcome {
    Outcome::Ready(1)
}

fn main() {
    let _ = rvs_make();
}
