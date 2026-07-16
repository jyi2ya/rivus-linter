#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Holder {
    result: Result<(), u8>,
}

fn rvs_process_M(data: String, holder: &mut Holder) -> Result<(), u8> {
    drop(data);
    let was_err = holder.result.is_err();
    let alias = &mut holder.result;
    *alias = Err(2);
    if was_err { Ok(()) } else { holder.result }
}

struct Pair {
    first: Result<(), u8>,
    second: Result<(), u8>,
}

fn rvs_preserve_sibling_projection_M(data: String, pair: &mut Pair) -> Result<(), u8> {
    drop(data);
    let first_was_err = pair.first.is_err();
    let alias = &mut pair.second;
    *alias = Err(2);
    if first_was_err { Ok(()) } else { pair.first }
}
