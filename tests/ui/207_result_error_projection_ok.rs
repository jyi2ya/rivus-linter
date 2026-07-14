// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Carry<T>(T);

trait ErrorType {
    type Error;
}

struct Marker;

impl ErrorType for Marker {
    type Error = Carry<String>;
}

fn rvs_process_projection(
    value: String,
) -> Result<(), <Marker as ErrorType>::Error> {
    Err(Carry(value))
}
