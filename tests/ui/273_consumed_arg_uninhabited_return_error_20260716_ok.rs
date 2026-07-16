// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_process(
    data: String,
    outcome: Option<std::convert::Infallible>,
) -> Result<(), std::convert::Infallible> {
    drop(data);
    match outcome {
        None => Ok(()),
        Some(error) => Err(error),
    }
}
