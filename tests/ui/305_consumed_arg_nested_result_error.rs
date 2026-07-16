#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let nested: Result<Result<(), u8>, u8> = Ok(Err(2));
    match nested {
        Ok(inner) => inner,
        Err(error) => Err(error),
    }
}
