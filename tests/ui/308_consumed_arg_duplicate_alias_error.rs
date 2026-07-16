#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Pair {
    changed: Result<(), u8>,
    observed: Result<(), u8>,
}

fn rvs_change_M(changed: &mut Result<(), u8>, observed: &Result<(), u8>) {
    *changed = Err(4);
    std::hint::black_box(observed);
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut pair = Pair {
        changed: Ok(()),
        observed: Ok(()),
    };
    pair.changed = Ok(());
    pair.observed = Ok(());
    rvs_change_M(&mut pair.changed, &pair.observed);
    pair.changed
}
